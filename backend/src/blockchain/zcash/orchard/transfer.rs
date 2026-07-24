//! Orchard privacy transfer implementation
//!
//! This module implements shielded (private) transfers using the Orchard protocol.
//! It supports:
//! - Shielded to shielded transfers (maximum privacy)
//! - Transparent to shielded transfers (shielding)
//! - Shielded to transparent transfers (deshielding)

#![allow(dead_code)]

use super::{
    constants::DEFAULT_FEE_ZATOSHIS,
    keys::OrchardSpendingKey,
    scanner::{OrchardNote, ShieldedBalance},
    OrchardError, OrchardResult,
};
use serde::{Deserialize, Serialize};

use orchard::{
    builder::{Builder as OrchardBuilder, BundleType, InProgress, Unauthorized},
    bundle::{BundleVersion, TxVersion},
    circuit::{OrchardCircuitVersion, ProvingKey},
    keys::{FullViewingKey, SpendAuthorizingKey},
    note::NoteVersion,
    tree::{Anchor, MerklePath},
    value::NoteValue,
    Proof,
};
use rand::rngs::OsRng;
use std::sync::OnceLock;
use incrementalmerkletree::Hashable;
use pasta_curves::{group::ff::PrimeField, pallas};

// Orchard merkle tree depth (from Zcash protocol)
const ORCHARD_MERKLE_DEPTH: usize = 32;

use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};

// ZIP 244 sighash personalization strings
const ZCASH_TRANSPARENT_HASH: &[u8] = b"ZTxIdTranspaHash";
const ZCASH_PREVOUTS_HASH: &[u8] = b"ZTxIdPrevoutHash";
const ZCASH_SEQUENCE_HASH: &[u8] = b"ZTxIdSequencHash";
const ZCASH_OUTPUTS_HASH: &[u8] = b"ZTxIdOutputsHash";
const ZCASH_TX_HASH: &[u8] = b"ZcashTxHash_";
const ZCASH_TRANSPARENT_SIG: &[u8] = b"Zcash___TxInHash";

// Transaction constants
const TX_VERSION_WITH_OVERWINTERED: u32 = 0x80000005;
const VERSION_GROUP_ID_V5: u32 = 0x26A7270A;

/// Global proving keys (expensive to build, so we cache them). There are two
/// circuits: FixedPostNu6_2 (old Orchard pool, pre-NU6.3) and PostNu6_3 (shared
/// by both the Orchard-v3 spend and Ironwood pools at NU6.3+). Cache both so the
/// turnstile path doesn't pay a ~20s build at activation time.
static ORCHARD_PROVING_KEY_V2: OnceLock<ProvingKey> = OnceLock::new();
static ORCHARD_PROVING_KEY_V3: OnceLock<ProvingKey> = OnceLock::new();

/// Initialize the pre-NU6.3 Orchard proving key (call at startup to avoid the
/// first-transfer delay). The PostNu6_3 key is built lazily on first turnstile
/// use; it can also be warmed via [`get_proving_key_for`].
/// This is an expensive operation (~20 seconds) but only needs to be done once.
pub fn init_proving_key() {
    let _ = get_proving_key_for(OrchardCircuitVersion::FixedPostNu6_2);
}

/// Get or build the proving key for a specific circuit version, cached per
/// circuit. FixedPostNu6_2 = old Orchard pool; PostNu6_3 = NU6.3+ (Orchard-v3
/// spend and Ironwood share this circuit, per orchard's `circuit_version()`).
fn get_proving_key_for(circuit: OrchardCircuitVersion) -> &'static ProvingKey {
    match circuit {
        OrchardCircuitVersion::PostNu6_3 => ORCHARD_PROVING_KEY_V3.get_or_init(|| {
            tracing::info!("Building Orchard PostNu6_3 proving key (turnstile/Ironwood)...");
            let pk = ProvingKey::build(OrchardCircuitVersion::PostNu6_3);
            tracing::info!("Orchard PostNu6_3 proving key built");
            pk
        }),
        // FixedPostNu6_2 and any earlier circuit map to the pre-NU6.3 key.
        _ => ORCHARD_PROVING_KEY_V2.get_or_init(|| {
            tracing::info!("Building Orchard FixedPostNu6_2 proving key...");
            let pk = ProvingKey::build(OrchardCircuitVersion::FixedPostNu6_2);
            tracing::info!("Orchard FixedPostNu6_2 proving key built");
            pk
        }),
    }
}

/// Which protocol era the chain is in, decided by the NU6.3 activation height.
/// Below NU6.3 (or if no NU6.3 is scheduled) the old Orchard-pool path applies
/// unchanged; at/above it, old-pool spends cross the turnstile into Ironwood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolEra {
    /// Pre-NU6.3: Orchard pool, note V2, tx V5, FixedPostNu6_2 circuit.
    PreNu63,
    /// NU6.3 active: Orchard→Ironwood turnstile, note V3 outputs, tx V6,
    /// PostNu6_3 circuit.
    PostNu63,
}

impl ProtocolEra {
    /// Decide the era from the current chain height and the (optional) NU6.3
    /// activation height read from the node. `None` (no NU6.3 scheduled) or a
    /// height below activation ⇒ pre-NU6.3 (the current, zero-change behaviour).
    pub fn from_heights(current_height: u64, nu63_activation: Option<u64>) -> Self {
        match nu63_activation {
            Some(h) if current_height >= h => ProtocolEra::PostNu63,
            _ => ProtocolEra::PreNu63,
        }
    }

    /// The bundle version used when *spending* existing Orchard-pool notes in
    /// this era. Pre-NU6.3: `orchard_v2()` (current). Post-NU6.3: `orchard_v3()`
    /// (turnstile spend; outputs are routed into an Ironwood bundle).
    fn orchard_spend_bundle_version(&self) -> BundleVersion {
        match self {
            ProtocolEra::PreNu63 => BundleVersion::orchard_v2(),
            ProtocolEra::PostNu63 => BundleVersion::orchard_v3(),
        }
    }

    /// The transaction version for this era: V5 pre-NU6.3, V6 at/after (Ironwood
    /// bundles cannot be committed with V5).
    fn tx_version(&self) -> TxVersion {
        match self {
            ProtocolEra::PreNu63 => TxVersion::V5,
            ProtocolEra::PostNu63 => TxVersion::V6,
        }
    }
}

/// Fund source for a transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FundSource {
    /// Automatically select funds (prefer shielded, fallback to transparent)
    Auto,
    /// Only use shielded funds
    Shielded,
    /// Only use transparent funds (shielding operation)
    Transparent,
}

impl Default for FundSource {
    fn default() -> Self {
        FundSource::Auto
    }
}

/// Transfer request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    /// Source wallet ID
    pub wallet_id: i32,
    /// Recipient address (unified or transparent)
    pub to_address: String,
    /// Amount in ZEC (decimal string)
    pub amount_zec: String,
    /// Amount in zatoshis (1 ZEC = 100,000,000 zatoshis)
    pub amount_zatoshis: Option<u64>,
    /// Optional encrypted memo (max 512 bytes)
    pub memo: Option<String>,
    /// Fund source preference
    #[serde(default)]
    pub fund_source: FundSource,
}

impl TransferRequest {
    /// Get amount in zatoshis
    pub fn get_zatoshis(&self) -> OrchardResult<u64> {
        if let Some(zatoshis) = self.amount_zatoshis {
            return Ok(zatoshis);
        }

        let zec: f64 = self.amount_zec.parse()
            .map_err(|_| OrchardError::TransactionBuild("Invalid amount format".to_string()))?;

        if zec <= 0.0 {
            return Err(OrchardError::TransactionBuild("Amount must be positive".to_string()));
        }

        Ok((zec * 100_000_000.0) as u64)
    }
}

/// Result of initiating a transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProposal {
    /// Unique proposal ID
    pub proposal_id: String,
    /// Amount to send (zatoshis)
    pub amount_zatoshis: u64,
    /// Estimated fee (zatoshis)
    pub fee_zatoshis: u64,
    /// Source of funds
    pub fund_source: FundSource,
    /// Whether this is a shielding operation (T → Z)
    pub is_shielding: bool,
    /// Whether this is a deshielding operation (Z → T)
    pub is_deshielding: bool,
    /// Recipient address
    pub to_address: String,
    /// Memo if provided
    pub memo: Option<String>,
    /// Expiry height for the transaction
    pub expiry_height: u64,
}

/// Result of executing a transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    /// Transaction ID (hash)
    pub tx_id: String,
    /// Transaction status
    pub status: TransferStatus,
    /// Raw transaction hex (for broadcasting)
    pub raw_tx: Option<String>,
    /// Amount sent (zatoshis)
    pub amount_zatoshis: u64,
    /// Fee paid (zatoshis)
    pub fee_zatoshis: u64,
}

/// Transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    /// Proposal created, awaiting execution
    Pending,
    /// Transaction built and signed
    Signed,
    /// Transaction submitted to network
    Submitted,
    /// Transaction confirmed on chain
    Confirmed,
    /// Transaction failed
    Failed,
}

/// Orchard transfer service
pub struct OrchardTransferService {
    /// Network parameters
    network: NetworkType,
    /// Consensus branch id read live from the node's chain tip. When set it
    /// overrides the network's hardcoded default so the shielded-tx sighash
    /// matches whatever network upgrade the chain is actually at (NU6.2,
    /// regtest/testnet, future NU7…). `None` falls back to the network default.
    consensus_branch_id: Option<u32>,
    /// NU6.3 (Ironwood) activation height read from the node, or None if not
    /// scheduled. With the current chain height this decides the [`ProtocolEra`]
    /// (pre-NU6.3 old-pool path vs the Orchard→Ironwood turnstile).
    nu63_activation_height: Option<u64>,
}

/// Network type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    Mainnet,
    Testnet,
}

impl NetworkType {
    /// Get the consensus branch ID for the current network upgrade (NU6.1)
    /// NU6.1 activated at height 3,146,400 on mainnet
    pub fn consensus_branch_id(&self) -> u32 {
        match self {
            NetworkType::Mainnet => 0x4dec4df0, // NU6.1 mainnet
            NetworkType::Testnet => 0x4dec4df0, // NU6.1 testnet
        }
    }

    /// Get the activation height for Orchard
    pub fn orchard_activation_height(&self) -> u64 {
        match self {
            NetworkType::Mainnet => 1687104, // NU5 activation on mainnet
            NetworkType::Testnet => 1842420, // NU5 activation on testnet
        }
    }
}

impl OrchardTransferService {
    /// Create a new transfer service (uses the network's default branch id)
    pub fn new(network: NetworkType) -> Self {
        Self {
            network,
            consensus_branch_id: None,
            nu63_activation_height: None,
        }
    }

    /// Create a transfer service pinned to a live consensus branch id read from
    /// the node. Use this for building/broadcasting so the sighash matches the
    /// chain's active network upgrade instead of a hardcoded constant.
    pub fn new_with_branch_id(network: NetworkType, consensus_branch_id: u32) -> Self {
        Self {
            network,
            consensus_branch_id: Some(consensus_branch_id),
            nu63_activation_height: None,
        }
    }

    /// Set the NU6.3 activation height (read from the node) so the builder can
    /// decide the [`ProtocolEra`]. `None` ⇒ turnstile never engages (pre-NU6.3
    /// behaviour). Builder-style; chains off the constructors.
    pub fn with_nu63_activation(mut self, nu63_activation_height: Option<u64>) -> Self {
        self.nu63_activation_height = nu63_activation_height;
        self
    }

    /// Decide the protocol era for a given chain height using the configured
    /// NU6.3 activation height.
    fn protocol_era(&self, current_height: u64) -> ProtocolEra {
        ProtocolEra::from_heights(current_height, self.nu63_activation_height)
    }

    /// Effective consensus branch id: the live value if pinned, else the
    /// network's hardcoded default.
    fn effective_branch_id(&self) -> u32 {
        self.consensus_branch_id
            .unwrap_or_else(|| self.effective_branch_id())
    }

    /// Create a transfer proposal
    ///
    /// This validates the request and calculates fees without building the transaction.
    pub fn create_proposal(
        &self,
        request: &TransferRequest,
        transparent_balance_zatoshis: u64,
        shielded_balance: Option<&ShieldedBalance>,
        current_height: u64,
    ) -> OrchardResult<TransferProposal> {
        let amount = request.get_zatoshis()?;

        // Check if target address is transparent (deshielding operation)
        let is_deshielding = is_transparent_address(&request.to_address);

        // Determine effective fund source and validate balance
        let (fund_source, is_shielding) = if is_deshielding {
            // Deshielding: must use shielded funds to send to transparent address
            let shielded_available = shielded_balance.map(|b| b.spendable_zatoshis).unwrap_or(0);
            if shielded_available == 0 {
                return Err(OrchardError::TransactionBuild(
                    "Deshielding requires shielded balance but none is available".to_string()
                ));
            }
            (FundSource::Shielded, false)
        } else {
            self.determine_fund_source(
                request.fund_source,
                amount,
                transparent_balance_zatoshis,
                shielded_balance,
            )?
        };

        // Calculate fee based on action count
        // For deshielding, we have 1 transparent output which must be included in fee calculation
        let fee = if is_deshielding {
            self.calculate_fee_with_transparent_outputs(1, fund_source, 1) // 1 transparent output
        } else {
            self.calculate_fee(1, fund_source)
        };
        let total_needed = amount + fee;

        // Validate sufficient funds
        let available = match fund_source {
            FundSource::Transparent => transparent_balance_zatoshis,
            FundSource::Shielded => shielded_balance.map(|b| b.spendable_zatoshis).unwrap_or(0),
            FundSource::Auto => {
                let shielded = shielded_balance.map(|b| b.spendable_zatoshis).unwrap_or(0);
                shielded + transparent_balance_zatoshis
            }
        };

        if available < total_needed {
            return Err(OrchardError::InsufficientBalance {
                available,
                required: total_needed,
            });
        }

        // Generate proposal ID
        let proposal_id = self.generate_proposal_id();

        // Calculate expiry height (default: current + 40 blocks, ~40 minutes)
        let expiry_height = current_height + 40;

        Ok(TransferProposal {
            proposal_id,
            amount_zatoshis: amount,
            fee_zatoshis: fee,
            fund_source,
            is_shielding,
            is_deshielding,
            to_address: request.to_address.clone(),
            memo: request.memo.clone(),
            expiry_height,
        })
    }

    /// Build and sign a transfer transaction
    ///
    /// This creates the actual Orchard transaction with proofs.
    ///
    /// # Arguments
    /// * `proposal` - The transfer proposal
    /// * `spending_key` - Orchard spending key for shielded operations
    /// * `private_key_hex` - Private key (hex) for signing transparent inputs
    /// * `spendable_notes` - Available shielded notes to spend
    /// * `transparent_inputs` - Transparent UTXOs to shield
    /// * `anchor_height` - Block height for anchor
    /// * `anchor` - Merkle tree anchor for Orchard
    pub fn build_transaction(
        &self,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        private_key_hex: &str,
        spendable_notes: Vec<(OrchardNote, MerklePath)>,  // Now includes MerklePath directly
        transparent_inputs: Vec<TransparentInput>,
        anchor_height: u64,
        anchor: Anchor,  // Now uses orchard::tree::Anchor directly
    ) -> OrchardResult<TransferResult> {
        // Log all proposal details for debugging
        tracing::info!(
            "build_transaction called: proposal_id={}, amount_zatoshis={}, fee_zatoshis={}, \
             fund_source={:?}, is_shielding={}, to_address={}",
            proposal.proposal_id,
            proposal.amount_zatoshis,
            proposal.fee_zatoshis,
            proposal.fund_source,
            proposal.is_shielding,
            &proposal.to_address[..std::cmp::min(20, proposal.to_address.len())]
        );

        // CRITICAL SAFETY CHECK: Prevent zero-value transactions
        if proposal.amount_zatoshis == 0 {
            tracing::error!(
                "BLOCKED in build_transaction: amount_zatoshis=0! proposal={}",
                proposal.proposal_id
            );
            return Err(OrchardError::TransactionBuild(
                "Cannot build transaction with zero amount".to_string()
            ));
        }

        // Protocol-era gate. Below NU6.3 the existing Orchard-pool path applies
        // unchanged (zero behaviour change). At/after NU6.3 an old-pool spend must
        // cross the turnstile into Ironwood (orchard_v3 spend + ironwood_v3
        // output, v6 transaction): P1.2 routes it to the dedicated turnstile
        // builder, leaving the pre-NU6.3 v5 path below completely untouched. On
        // mainnet this branch is unreachable until the 7/28 NU6.3 activation; on
        // regtest 档B it engages at the configured NU6.3 height.
        let era = self.protocol_era(anchor_height);
        if era == ProtocolEra::PostNu63 {
            let nu63_activation = self.nu63_activation_height.ok_or_else(|| {
                OrchardError::TransactionBuild(
                    "PostNu63 era decided but NU6.3 activation height is missing".to_string(),
                )
            })?;
            return self.build_turnstile_result(
                proposal,
                spending_key,
                spendable_notes,
                anchor_height,
                anchor,
                nu63_activation,
            );
        }

        // Log transparent inputs
        let total_input: u64 = transparent_inputs.iter().map(|i| i.value).sum();
        tracing::info!(
            "Transparent inputs: count={}, total_value={} zatoshis",
            transparent_inputs.len(),
            total_input
        );

        // Validate that total input covers amount + fee
        if proposal.is_shielding && total_input < proposal.amount_zatoshis + proposal.fee_zatoshis {
            tracing::error!(
                "Insufficient inputs: have {} zatoshis, need {} zatoshis (amount={} + fee={})",
                total_input,
                proposal.amount_zatoshis + proposal.fee_zatoshis,
                proposal.amount_zatoshis,
                proposal.fee_zatoshis
            );
            return Err(OrchardError::InsufficientBalance {
                available: total_input,
                required: proposal.amount_zatoshis + proposal.fee_zatoshis,
            });
        }

        // Validate inputs
        if proposal.fund_source == FundSource::Shielded && spendable_notes.is_empty() {
            return Err(OrchardError::NoSpendableNotes);
        }

        if proposal.fund_source == FundSource::Transparent && transparent_inputs.is_empty() {
            return Err(OrchardError::TransactionBuild(
                "No transparent inputs provided for shielding".to_string()
            ));
        }

        // Build the transaction
        let tx_data = self.build_orchard_transaction(
            proposal,
            spending_key,
            private_key_hex,
            spendable_notes,
            transparent_inputs,
            anchor_height,
            anchor,
        )?;

        // Generate transaction ID
        let tx_id = self.compute_tx_id(&tx_data);

        Ok(TransferResult {
            tx_id,
            status: TransferStatus::Signed,
            raw_tx: Some(hex::encode(&tx_data)),
            amount_zatoshis: proposal.amount_zatoshis,
            fee_zatoshis: proposal.fee_zatoshis,
        })
    }

    /// F4.0-c P1.2c: build the real NU6.3/Ironwood turnstile transaction for a
    /// shielded (old Orchard pool) spend and return the signed raw tx. Engaged
    /// only at [`ProtocolEra::PostNu63`] from [`Self::build_transaction`].
    ///
    /// Semantics (see `turnstile` module docs): we spend old-pool V2 notes (value
    /// out via the bundle's value balance) and route ALL value — payment + change
    /// — into the Ironwood pool. That is the migration semantics and it also
    /// avoids the old pool's post-NU6.3 *cross-address* restriction (a
    /// cross-address old-pool output is invalid; same-address change would be
    /// legal, but we cross everything to Ironwood regardless).
    /// The heavy v6/sighash/proof/value-balance work is delegated to
    /// `zcash_primitives`' Builder via [`build_turnstile_transaction`]; this
    /// method only reconstructs notes (with the same fail-closed nullifier guard
    /// as the v5 path) and shapes the Ironwood outputs. The pre-NU6.3 v5 path is
    /// left completely untouched.
    ///
    /// This round wires the shielded-spend turnstile (migration + shielded batch
    /// transfer). Shielding (T→Z) and deshielding (Z→T to a transparent address)
    /// after NU6.3 are a distinct (transparent-side) construction and fail closed
    /// here with a clear message rather than being mis-built.
    fn build_turnstile_result(
        &self,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        spendable_notes: Vec<(OrchardNote, MerklePath)>,
        anchor_height: u64,
        anchor: Anchor,
        nu63_activation: u64,
    ) -> OrchardResult<TransferResult> {
        use super::address::OrchardAddressManager;
        use super::turnstile::{
            build_turnstile_transaction, TurnstileOutput, ZpayNetworkParams,
        };
        use orchard::keys::{Diversifier, Scope};
        use zcash_protocol::consensus::NetworkUpgrade;
        use zcash_protocol::memo::MemoBytes;

        // Out-of-scope-this-round constructions fail closed (never mis-built).
        if is_transparent_address(&proposal.to_address) {
            return Err(OrchardError::TransactionBuild(
                "post-NU6.3 deshielding (shielded → transparent) turnstile is not yet wired; \
                 refusing to build".to_string(),
            ));
        }
        if proposal.fund_source == FundSource::Transparent {
            return Err(OrchardError::TransactionBuild(
                "post-NU6.3 shielding (transparent → Ironwood) is not yet wired; \
                 refusing to build".to_string(),
            ));
        }
        if spendable_notes.is_empty() {
            return Err(OrchardError::NoSpendableNotes);
        }

        let amount = proposal.amount_zatoshis;
        let fee = proposal.fee_zatoshis;
        let total_needed = amount + fee;

        // Select old-pool notes to cover amount + fee.
        let (selected, total_input) =
            self.select_notes_with_paths(spendable_notes, total_needed)?;
        let change_amount = total_input - total_needed;

        tracing::info!(
            "Turnstile build: {} old-pool notes selected, total_input={} zat, amount={} zat, \
             fee={} zat, change={} zat → Ironwood",
            selected.len(),
            total_input,
            amount,
            fee,
            change_amount
        );

        let fvk = spending_key.to_fvk();
        let ovk = Some(spending_key.to_ovk());

        // Reconstruct each selected old-pool V2 note + fail-closed nullifier
        // guard (identical discipline to the v5 path: stored rho/rseed
        // inconsistent with the note would build a spend the node rejects —
        // catch it here with a precise error rather than broadcasting a doomed tx).
        let mut spends: Vec<(FullViewingKey, orchard::Note, MerklePath)> =
            Vec::with_capacity(selected.len());
        let mut saks: Vec<SpendAuthorizingKey> = Vec::with_capacity(selected.len());
        for (idx, (note, merkle_path)) in selected.iter().enumerate() {
            let recipient_addr = orchard::Address::from_raw_address_bytes(&note.recipient);
            if recipient_addr.is_none().into() {
                return Err(OrchardError::TransactionBuild(format!(
                    "Invalid recipient address data for note {idx}"
                )));
            }
            let recipient_addr = recipient_addr.unwrap();

            let rho = orchard::note::Rho::from_bytes(&note.rho);
            if rho.is_none().into() {
                return Err(OrchardError::TransactionBuild(format!(
                    "Invalid rho data for note {idx}"
                )));
            }
            let rho = rho.unwrap();

            let rseed = orchard::note::RandomSeed::from_bytes(note.rseed, &rho);
            if rseed.is_none().into() {
                return Err(OrchardError::TransactionBuild(format!(
                    "Invalid rseed data for note {idx}"
                )));
            }
            let rseed = rseed.unwrap();

            // Old-pool notes are V2 (rcm_v2); the Ironwood outputs are V3.
            let value = NoteValue::from_raw(note.value_zatoshis);
            let orchard_note =
                orchard::Note::from_parts(recipient_addr, value, rho, rseed, NoteVersion::V2);
            if orchard_note.is_none().into() {
                return Err(OrchardError::TransactionBuild(format!(
                    "Failed to reconstruct Orchard note {idx}"
                )));
            }
            let orchard_note = orchard_note.unwrap();

            let recomputed_nf = orchard_note.nullifier(&fvk).to_bytes();
            if recomputed_nf != note.nullifier {
                return Err(OrchardError::TransactionBuild(format!(
                    "Note {idx} reconstruction mismatch: recomputed nullifier {} != stored {} \
                     (stored rho/rseed inconsistent with note — re-scan required)",
                    hex::encode(recomputed_nf),
                    hex::encode(note.nullifier)
                )));
            }

            spends.push((fvk.clone(), orchard_note, merkle_path.clone()));
            saks.push(SpendAuthorizingKey::from(spending_key.sk()));
        }

        // Ironwood outputs: payment to the recipient + change to our own internal
        // Ironwood address. No old-pool output at all — everything crosses to the
        // new pool (migration semantics; also avoids the old pool's post-NU6.3
        // cross-address restriction). The sum below equals total_input − fee,
        // which is exactly the value balance the builder needs.
        let recipient = OrchardAddressManager::extract_orchard_address(&proposal.to_address)?;
        let payment_memo = match proposal.memo.as_ref() {
            Some(m) if !m.is_empty() => MemoBytes::from_bytes(m.as_bytes()).map_err(|e| {
                OrchardError::TransactionBuild(format!("invalid memo: {e:?}"))
            })?,
            _ => MemoBytes::empty(),
        };
        let mut outputs = vec![TurnstileOutput {
            ovk: ovk.clone(),
            recipient,
            value_zatoshis: amount,
            memo: payment_memo,
        }];
        if change_amount > 0 {
            let change_addr =
                fvk.address(Diversifier::from_bytes([0u8; 11]), Scope::Internal);
            outputs.push(TurnstileOutput {
                ovk,
                recipient: change_addr,
                value_zatoshis: change_amount,
                memo: MemoBytes::empty(),
            });
        }

        // Mirror the node's NU6.3 activation so Builder::new derives
        // orchard_v3 + ironwood_v3 + tx v6 for the target (anchor) height.
        // The local NetworkType (Mainnet/Testnet) maps onto zcash_protocol's.
        let zcash_network = match self.network {
            NetworkType::Mainnet => zcash_protocol::consensus::NetworkType::Main,
            NetworkType::Testnet => zcash_protocol::consensus::NetworkType::Test,
        };
        let params = ZpayNetworkParams::new(
            zcash_network,
            vec![(NetworkUpgrade::Nu6_3, nu63_activation)],
        );

        let built = build_turnstile_transaction(
            params,
            anchor_height,
            anchor,
            spends,
            saks,
            outputs,
            fee,
        )
        .map_err(|e| OrchardError::TransactionBuild(format!("turnstile build failed: {e}")))?;

        tracing::info!(
            "Turnstile transaction built: txid={}, raw_len={} bytes",
            built.txid,
            built.raw_tx.len()
        );

        Ok(TransferResult {
            tx_id: built.txid,
            status: TransferStatus::Signed,
            raw_tx: Some(hex::encode(&built.raw_tx)),
            amount_zatoshis: amount,
            fee_zatoshis: fee,
        })
    }

    /// Determine the effective fund source based on availability
    fn determine_fund_source(
        &self,
        requested: FundSource,
        amount: u64,
        transparent_balance: u64,
        shielded_balance: Option<&ShieldedBalance>,
    ) -> OrchardResult<(FundSource, bool)> {
        let shielded_available = shielded_balance
            .map(|b| b.spendable_zatoshis)
            .unwrap_or(0);

        // Estimate minimum fee
        let min_fee = DEFAULT_FEE_ZATOSHIS;
        let total_needed = amount + min_fee;

        match requested {
            FundSource::Shielded => {
                // Shielded spending: requires spending data (recipient, rho, rseed) stored during scanning
                if shielded_available < total_needed {
                    return Err(OrchardError::InsufficientBalance {
                        available: shielded_available,
                        required: total_needed,
                    });
                }
                Ok((FundSource::Shielded, false)) // Not a shielding operation
            }
            FundSource::Transparent => {
                if transparent_balance < total_needed {
                    return Err(OrchardError::InsufficientBalance {
                        available: transparent_balance,
                        required: total_needed,
                    });
                }
                Ok((FundSource::Transparent, true)) // Shielding operation
            }
            FundSource::Auto => {
                // Prefer shielded funds for better privacy, fall back to transparent
                if shielded_available >= total_needed {
                    Ok((FundSource::Shielded, false))
                } else if transparent_balance >= total_needed {
                    Ok((FundSource::Transparent, true)) // Shielding operation
                } else {
                    Err(OrchardError::InsufficientBalance {
                        available: shielded_available + transparent_balance,
                        required: total_needed,
                    })
                }
            }
        }
    }

    /// Calculate transaction fee using ZIP-317 formula
    ///
    /// ZIP-317: fee ≥ marginal_fee × max(grace_actions, logical_actions)
    /// - marginal_fee = 5000 zatoshis
    /// - grace_actions = 2
    /// - logical_actions = transparent_inputs + transparent_outputs + orchard_actions
    fn calculate_fee(&self, num_outputs: u32, fund_source: FundSource) -> u64 {
        self.calculate_fee_with_transparent_outputs(num_outputs, fund_source, 0)
    }

    /// Calculate transaction fee with explicit transparent output count (for deshielding)
    fn calculate_fee_with_transparent_outputs(&self, num_outputs: u32, fund_source: FundSource, transparent_outputs: u32) -> u64 {
        // ZIP-317 constants
        const MARGINAL_FEE: u64 = 5000;
        const GRACE_ACTIONS: u32 = 2;

        // Calculate logical actions based on fund source
        let (transparent_inputs, orchard_actions) = match fund_source {
            FundSource::Shielded => {
                // Pure shielded: no transparent inputs
                // Orchard actions = max(spends, outputs), minimum 2 due to padding
                // With payment + change = 2 outputs, need 2 actions
                (0u32, std::cmp::max(2, num_outputs + 1)) // +1 for potential change
            }
            FundSource::Transparent => {
                // Shielding: transparent inputs + Orchard outputs
                // Assume 1 transparent input (will be adjusted if more UTXOs needed)
                // Orchard outputs = payment + change = 2, so 2 actions
                (1u32, std::cmp::max(2, num_outputs + 1)) // +1 for change
            }
            FundSource::Auto => {
                // Could be either, assume worst case (shielding with change)
                (1u32, std::cmp::max(2, num_outputs + 1))
            }
        };

        // logical_actions includes transparent inputs, transparent outputs, and orchard actions
        let logical_actions = transparent_inputs + transparent_outputs + orchard_actions;
        let fee = MARGINAL_FEE * std::cmp::max(GRACE_ACTIONS, logical_actions) as u64;

        tracing::debug!(
            "ZIP-317 fee calculation: transparent_inputs={}, transparent_outputs={}, orchard_actions={}, logical_actions={}, fee={}",
            transparent_inputs,
            transparent_outputs,
            orchard_actions,
            logical_actions,
            fee
        );

        fee
    }

    /// Generate a unique proposal ID
    fn generate_proposal_id(&self) -> String {
        use rand::RngCore;
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    /// Build the actual Orchard transaction
    fn build_orchard_transaction(
        &self,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        private_key_hex: &str,
        spendable_notes: Vec<(OrchardNote, MerklePath)>,  // Now includes MerklePath directly
        transparent_inputs: Vec<TransparentInput>,
        _anchor_height: u64,
        anchor: Anchor,  // Now uses orchard::tree::Anchor directly
    ) -> OrchardResult<Vec<u8>> {
        // Create transaction builder
        let mut tx_data = Vec::new();

        // Transaction header (v5 format for Orchard support)
        // Version: 5 with overwinter flag (0x80000005 in little-endian)
        const TX_VERSION_V5_OVERWINTERED: u32 = 0x80000005;
        tx_data.extend_from_slice(&TX_VERSION_V5_OVERWINTERED.to_le_bytes());

        // Version group ID for v5 (0x26A7270A in little-endian)
        const VERSION_GROUP_ID_V5: u32 = 0x26A7270A;
        tx_data.extend_from_slice(&VERSION_GROUP_ID_V5.to_le_bytes());

        // Consensus branch ID
        tx_data.extend_from_slice(&self.effective_branch_id().to_le_bytes());

        // Lock time (0 = no lock)
        tx_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        // Expiry height
        tx_data.extend_from_slice(&(proposal.expiry_height as u32).to_le_bytes());

        // Check if this is a deshielding operation (Z → T)
        let is_deshielding = is_transparent_address(&proposal.to_address);

        // Build the appropriate bundle based on fund source and target address
        if is_deshielding {
            // Deshielding: Shielded to transparent transfer
            tracing::info!(
                "Building deshielding transaction: Z → T, amount={} zatoshis to {}",
                proposal.amount_zatoshis,
                &proposal.to_address
            );
            self.build_deshielding_bundle(
                &mut tx_data,
                proposal,
                spending_key,
                spendable_notes,
                anchor,
            )?;
        } else {
            match proposal.fund_source {
                FundSource::Shielded => {
                    // Shielded to shielded transfer
                    self.build_shielded_bundle(
                        &mut tx_data,
                        proposal,
                        spending_key,
                        spendable_notes,
                        anchor,
                    )?;
                }
                FundSource::Transparent => {
                    // Transparent to shielded (shielding)
                    self.build_shielding_bundle(
                        &mut tx_data,
                        proposal,
                        spending_key,
                        private_key_hex,
                        transparent_inputs,
                        anchor,
                    )?;
                }
                FundSource::Auto => {
                    // Prefer shielded spending for better privacy
                    if !spendable_notes.is_empty() {
                        self.build_shielded_bundle(
                            &mut tx_data,
                            proposal,
                            spending_key,
                            spendable_notes,
                            anchor,
                        )?;
                    } else if !transparent_inputs.is_empty() {
                        // Fall back to transparent (shielding operation)
                        self.build_shielding_bundle(
                            &mut tx_data,
                            proposal,
                            spending_key,
                            private_key_hex,
                            transparent_inputs,
                            anchor,
                        )?;
                    } else {
                        return Err(OrchardError::TransactionBuild(
                            "No funds available: no transparent UTXOs or shielded notes found".to_string()
                        ));
                    }
                }
            }
        }

        Ok(tx_data)
    }

    /// Build shielded-to-shielded bundle (spending from shielded pool)
    ///
    /// This creates a transaction that spends from shielded notes and sends to shielded addresses.
    /// Now receives notes with their MerklePaths directly (using proper conversion from IncrementalWitness).
    fn build_shielded_bundle(
        &self,
        tx_data: &mut Vec<u8>,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        notes_with_paths: Vec<(OrchardNote, MerklePath)>,  // Notes with their MerklePaths
        anchor: Anchor,  // Anchor directly as orchard::tree::Anchor
    ) -> OrchardResult<()> {
        use super::address::OrchardAddressManager;
        use orchard::keys::Scope;
        use orchard::value::NoteValue;

        tracing::info!(
            "Building shielded bundle: {} notes to spend, amount={} zatoshis, fee={} zatoshis",
            notes_with_paths.len(),
            proposal.amount_zatoshis,
            proposal.fee_zatoshis
        );

        // Select notes to cover the required amount
        let total_needed = proposal.amount_zatoshis + proposal.fee_zatoshis;
        let (selected_notes_with_paths, total_input) = self.select_notes_with_paths(notes_with_paths, total_needed)?;

        tracing::info!(
            "Selected {} notes with total {} zatoshis (need {} zatoshis)",
            selected_notes_with_paths.len(),
            total_input,
            total_needed
        );

        // Calculate change
        let change_amount = total_input - total_needed;

        // Get the proving key
        let pk = get_proving_key_for(OrchardCircuitVersion::FixedPostNu6_2);

        // Get FVK from spending key
        let fvk = spending_key.to_fvk();

        // Create builder with the anchor directly
        let bundle_type = BundleType::DEFAULT;
        // F4.0-a 旧 Orchard 池:orchard_v2()(NU6.2→NU6.3 语义)+ 其 default_flags。
        // 双池接缝:Ironwood 改 BundleVersion::ironwood_v3(),旧池 NU6.3 后改 orchard_v3()。
        let bundle_version = BundleVersion::orchard_v2();
        let bundle_flags = bundle_version.default_flags();
        let mut builder = OrchardBuilder::new(bundle_type, bundle_version, bundle_flags, anchor)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create builder: {:?}", e)))?;

        // Add spends (reconstruct Note from stored data, use MerklePath directly)
        for (idx, (note, merkle_path)) in selected_notes_with_paths.iter().enumerate() {
            // Reconstruct the orchard::Address from stored bytes
            let recipient_addr = orchard::Address::from_raw_address_bytes(&note.recipient);
            if recipient_addr.is_none().into() {
                tracing::error!("Failed to reconstruct address for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid recipient address data for note {}", idx)
                ));
            }
            let recipient_addr = recipient_addr.unwrap();

            // Reconstruct Rho from stored bytes
            let rho = orchard::note::Rho::from_bytes(&note.rho);
            if rho.is_none().into() {
                tracing::error!("Failed to reconstruct rho for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid rho data for note {}", idx)
                ));
            }
            let rho = rho.unwrap();

            // Reconstruct RandomSeed from stored bytes
            let rseed = orchard::note::RandomSeed::from_bytes(note.rseed, &rho);
            if rseed.is_none().into() {
                tracing::error!("Failed to reconstruct rseed for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid rseed data for note {}", idx)
                ));
            }
            let rseed = rseed.unwrap();

            // Reconstruct the Note
            let value = NoteValue::from_raw(note.value_zatoshis);
            // F4.0-a 旧 Orchard 池 note = V2(rcm_v2)。双池接缝:Ironwood 用 NoteVersion::V3(rcm_v3)。
            let orchard_note = orchard::Note::from_parts(recipient_addr, value, rho, rseed, NoteVersion::V2);
            if orchard_note.is_none().into() {
                tracing::error!("Failed to reconstruct note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Failed to reconstruct Orchard note {}", idx)
                ));
            }
            let orchard_note = orchard_note.unwrap();

            // Fail-closed guard: recompute this note's nullifier from the
            // reconstructed note and require it to match the nullifier stored at
            // scan time. If the stored rho/rseed are inconsistent with the note,
            // add_spend would build a spend the node rejects (incorrect/duplicate
            // nullifier, -25). Catch it here with a precise error instead of
            // broadcasting a doomed transaction.
            let recomputed_nf = orchard_note.nullifier(&fvk).to_bytes();
            if recomputed_nf != note.nullifier {
                tracing::error!(
                    "Note {} reconstruction mismatch: recomputed nullifier {} != stored {}",
                    idx,
                    hex::encode(recomputed_nf),
                    hex::encode(note.nullifier)
                );
                return Err(OrchardError::TransactionBuild(format!(
                    "Note {} reconstruction mismatch: recomputed nullifier {} != stored {} \
                     (stored rho/rseed inconsistent with note — re-scan required)",
                    idx,
                    hex::encode(recomputed_nf),
                    hex::encode(note.nullifier)
                )));
            }

            let extracted_cmx = orchard::note::ExtractedNoteCommitment::from(orchard_note.commitment());
            let reconstructed_cmx = extracted_cmx.to_bytes();
            tracing::info!(
                "Note {} verified: nullifier matches stored, commitment {}, position={}",
                idx,
                hex::encode(&reconstructed_cmx[..8]),
                note.position
            );

            // Add the spend using the MerklePath directly (from proper conversion)
            match builder.add_spend(fvk.clone(), orchard_note, merkle_path.clone()) {
                Ok(_) => {
                    tracing::debug!(
                        "Added spend {}: value={} zatoshis",
                        idx,
                        note.value_zatoshis
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to add spend {}: {:?}", idx, e);
                    return Err(OrchardError::TransactionBuild(
                        format!("Failed to add spend: {:?}", e)
                    ));
                }
            }
        }

        // Add output: payment to recipient
        let recipient_address = OrchardAddressManager::extract_orchard_address(&proposal.to_address)?;
        let ovk = Some(spending_key.to_ovk());
        let payment_value = NoteValue::from_raw(proposal.amount_zatoshis);
        let memo_bytes: [u8; 512] = {
            let mut bytes = [0u8; 512];
            if let Some(m) = proposal.memo.as_ref() {
                let len = std::cmp::min(m.as_bytes().len(), 512);
                bytes[..len].copy_from_slice(&m.as_bytes()[..len]);
            }
            bytes
        };

        builder.add_output(ovk.clone(), recipient_address, payment_value, memo_bytes)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to add payment output: {:?}", e)))?;

        tracing::info!(
            "Added payment output: {} zatoshis to {}...",
            proposal.amount_zatoshis,
            &proposal.to_address[..std::cmp::min(20, proposal.to_address.len())]
        );

        // Add output: change to sender (if any)
        if change_amount > 0 {
            let change_diversifier = orchard::keys::Diversifier::from_bytes([0u8; 11]);
            let change_address = fvk.address(change_diversifier, Scope::Internal);
            let change_value = NoteValue::from_raw(change_amount);

            builder.add_output(ovk, change_address, change_value, [0u8; 512])
                .map_err(|e| OrchardError::TransactionBuild(format!("Failed to add change output: {:?}", e)))?;

            tracing::info!("Added change output: {} zatoshis", change_amount);
        }

        // Build the bundle
        tracing::info!("Building Orchard bundle...");
        let (unauthorized_bundle, _meta) = builder
            .build::<i64>(&mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to build bundle: {:?}", e)))?
            .ok_or_else(|| OrchardError::TransactionBuild("Empty bundle".to_string()))?;

        tracing::info!(
            "Bundle built successfully: {} actions in bundle",
            unauthorized_bundle.actions().len()
        );

        // Create proof
        tracing::info!("Creating Orchard proof (this may take a few seconds)...");
        let proof_start = std::time::Instant::now();
        let proven_bundle = unauthorized_bundle
            .create_proof(pk, &mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create proof: {:?}", e)))?;
        tracing::info!("Proof created in {:.2}s", proof_start.elapsed().as_secs_f64());

        // Compute proper sighash for signatures (ZIP 244)
        // For shielded-to-shielded, there are no transparent inputs
        let sighash = self.compute_shielded_sighash(
            &[], // no transparent inputs
            proposal.expiry_height as u32,
            self.effective_branch_id(),
            &proven_bundle,
        )?;
        tracing::info!("Computed shielded sighash: {}", hex::encode(&sighash));

        // Apply signatures (spend auth + binding)
        let saks: Vec<orchard::keys::SpendAuthorizingKey> = selected_notes_with_paths
            .iter()
            .map(|_| orchard::keys::SpendAuthorizingKey::from(spending_key.sk()))
            .collect();

        tracing::info!("Applying {} spend authorization signatures", saks.len());
        let authorized_bundle = proven_bundle
            .apply_signatures(OsRng, sighash, &saks)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to apply signatures: {:?}", e)))?;
        tracing::info!("Signatures applied successfully");

        // Serialize the transaction
        // No transparent inputs/outputs for shielded-to-shielded
        tx_data.push(0x00); // vin count
        tx_data.push(0x00); // vout count
        tx_data.push(0x00); // nSpendsSapling
        tx_data.push(0x00); // nOutputsSapling

        // Serialize Orchard bundle
        self.serialize_orchard_bundle(&authorized_bundle, tx_data)?;

        tracing::info!(
            "Built shielded transaction: {} spends, {} outputs, {} bytes",
            selected_notes_with_paths.len(),
            if change_amount > 0 { 2 } else { 1 },
            tx_data.len()
        );

        Ok(())
    }

    /// Build deshielding bundle (shielded to transparent transfer, Z → T)
    ///
    /// This creates a transaction that spends from shielded notes and sends to transparent addresses.
    /// The recipient will receive funds at their transparent address (t1... or t3...)
    fn build_deshielding_bundle(
        &self,
        tx_data: &mut Vec<u8>,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        notes_with_paths: Vec<(OrchardNote, MerklePath)>,
        anchor: Anchor,
    ) -> OrchardResult<()> {
        use orchard::keys::Scope;
        use orchard::value::NoteValue;

        tracing::info!(
            "Building deshielding bundle: {} notes to spend, amount={} zatoshis, fee={} zatoshis, to={}",
            notes_with_paths.len(),
            proposal.amount_zatoshis,
            proposal.fee_zatoshis,
            &proposal.to_address
        );

        // Select notes to cover the required amount
        let total_needed = proposal.amount_zatoshis + proposal.fee_zatoshis;
        let (selected_notes_with_paths, total_input) = self.select_notes_with_paths(notes_with_paths, total_needed)?;

        tracing::info!(
            "Selected {} notes with total {} zatoshis (need {} zatoshis)",
            selected_notes_with_paths.len(),
            total_input,
            total_needed
        );

        // Calculate change (change goes back to shielded pool)
        let change_amount = total_input - total_needed;

        // Get the proving key
        let pk = get_proving_key_for(OrchardCircuitVersion::FixedPostNu6_2);

        // Get FVK from spending key
        let fvk = spending_key.to_fvk();

        // Create builder with the anchor
        let bundle_type = BundleType::DEFAULT;
        // F4.0-a 旧 Orchard 池:orchard_v2()(NU6.2→NU6.3 语义)+ 其 default_flags。
        // 双池接缝:Ironwood 改 BundleVersion::ironwood_v3(),旧池 NU6.3 后改 orchard_v3()。
        let bundle_version = BundleVersion::orchard_v2();
        let bundle_flags = bundle_version.default_flags();
        let mut builder = OrchardBuilder::new(bundle_type, bundle_version, bundle_flags, anchor)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create builder: {:?}", e)))?;

        // Add spends (from shielded notes)
        for (idx, (note, merkle_path)) in selected_notes_with_paths.iter().enumerate() {
            // Reconstruct the orchard::Address from stored bytes
            let recipient_addr = orchard::Address::from_raw_address_bytes(&note.recipient);
            if recipient_addr.is_none().into() {
                tracing::error!("Failed to reconstruct address for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid recipient address data for note {}", idx)
                ));
            }
            let recipient_addr = recipient_addr.unwrap();

            // Reconstruct Rho from stored bytes
            let rho = orchard::note::Rho::from_bytes(&note.rho);
            if rho.is_none().into() {
                tracing::error!("Failed to reconstruct rho for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid rho data for note {}", idx)
                ));
            }
            let rho = rho.unwrap();

            // Reconstruct RandomSeed from stored bytes
            let rseed = orchard::note::RandomSeed::from_bytes(note.rseed, &rho);
            if rseed.is_none().into() {
                tracing::error!("Failed to reconstruct rseed for note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Invalid rseed data for note {}", idx)
                ));
            }
            let rseed = rseed.unwrap();

            // Reconstruct the Note
            let value = NoteValue::from_raw(note.value_zatoshis);
            // F4.0-a 旧 Orchard 池 note = V2(rcm_v2)。双池接缝:Ironwood 用 NoteVersion::V3(rcm_v3)。
            let orchard_note = orchard::Note::from_parts(recipient_addr, value, rho, rseed, NoteVersion::V2);
            if orchard_note.is_none().into() {
                tracing::error!("Failed to reconstruct note {}", idx);
                return Err(OrchardError::TransactionBuild(
                    format!("Failed to reconstruct Orchard note {}", idx)
                ));
            }
            let orchard_note = orchard_note.unwrap();

            // Fail-closed guard: reconstructed nullifier must match the stored one
            // (see the shielding path for rationale). Prevents broadcasting a spend
            // the node rejects with -25 when stored rho/rseed are inconsistent.
            let recomputed_nf = orchard_note.nullifier(&fvk).to_bytes();
            if recomputed_nf != note.nullifier {
                tracing::error!(
                    "Note {} reconstruction mismatch: recomputed nullifier {} != stored {}",
                    idx,
                    hex::encode(recomputed_nf),
                    hex::encode(note.nullifier)
                );
                return Err(OrchardError::TransactionBuild(format!(
                    "Note {} reconstruction mismatch: recomputed nullifier {} != stored {} \
                     (stored rho/rseed inconsistent with note — re-scan required)",
                    idx,
                    hex::encode(recomputed_nf),
                    hex::encode(note.nullifier)
                )));
            }

            // Add the spend
            match builder.add_spend(fvk.clone(), orchard_note, merkle_path.clone()) {
                Ok(_) => {
                    tracing::debug!(
                        "Added spend {}: value={} zatoshis",
                        idx,
                        note.value_zatoshis
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to add spend {}: {:?}", idx, e);
                    return Err(OrchardError::TransactionBuild(
                        format!("Failed to add spend: {:?}", e)
                    ));
                }
            }
        }

        // For deshielding, we need to add a dummy Orchard output to balance the bundle
        // The actual payment goes to transparent output
        // Add change output to sender (if any)
        let ovk = Some(spending_key.to_ovk());
        if change_amount > 0 {
            let change_diversifier = orchard::keys::Diversifier::from_bytes([0u8; 11]);
            let change_address = fvk.address(change_diversifier, Scope::Internal);
            let change_value = NoteValue::from_raw(change_amount);

            builder.add_output(ovk.clone(), change_address, change_value, [0u8; 512])
                .map_err(|e| OrchardError::TransactionBuild(format!("Failed to add change output: {:?}", e)))?;

            tracing::info!("Added shielded change output: {} zatoshis", change_amount);
        }

        // Build the Orchard bundle
        tracing::info!("Building Orchard bundle for deshielding...");
        let (unauthorized_bundle, _meta) = builder
            .build::<i64>(&mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to build bundle: {:?}", e)))?
            .ok_or_else(|| OrchardError::TransactionBuild("Empty bundle".to_string()))?;

        let vb = *unauthorized_bundle.value_balance();
        tracing::info!(
            "Bundle built successfully: {} actions in bundle, value_balance={} (expected: {} = payment {} + fee {})",
            unauthorized_bundle.actions().len(),
            vb,
            proposal.amount_zatoshis as i64 + proposal.fee_zatoshis as i64,
            proposal.amount_zatoshis,
            proposal.fee_zatoshis
        );

        // For deshielding, value_balance should be POSITIVE (funds flowing out of Orchard pool)
        // value_balance = sum(spends) - sum(outputs) = total_input - change = payment + fee
        let expected_vb = proposal.amount_zatoshis as i64 + proposal.fee_zatoshis as i64;
        if vb != expected_vb {
            tracing::error!(
                "CRITICAL: Orchard bundle value_balance mismatch! got={}, expected={}, total_input={}, change={}",
                vb, expected_vb, total_input, change_amount
            );
        }

        // Build transparent output for the recipient
        let transparent_output = self.build_transparent_output(&proposal.to_address, proposal.amount_zatoshis)?;

        // Create proof FIRST (following the same pattern as working Z→Z transfers)
        tracing::info!("Creating Orchard proof for deshielding...");
        let proof_start = std::time::Instant::now();
        let proven_bundle = unauthorized_bundle
            .create_proof(pk, &mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create proof: {:?}", e)))?;
        tracing::info!("Proof created in {:.2}s", proof_start.elapsed().as_secs_f64());

        // Compute sighash AFTER creating proof (from proven bundle, like Z→Z)
        // For deshielding, we need to include transparent outputs in the sighash
        let sighash = self.compute_deshielding_sighash(
            &transparent_output,
            proposal.expiry_height as u32,
            self.effective_branch_id(),
            &proven_bundle,
        )?;
        tracing::info!("Computed deshielding sighash: {}", hex::encode(&sighash));

        // Apply signatures
        let saks: Vec<orchard::keys::SpendAuthorizingKey> = selected_notes_with_paths
            .iter()
            .map(|_| orchard::keys::SpendAuthorizingKey::from(spending_key.sk()))
            .collect();

        tracing::info!("Applying {} spend authorization signatures", saks.len());
        let authorized_bundle = proven_bundle
            .apply_signatures(OsRng, sighash, &saks)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to apply signatures: {:?}", e)))?;
        tracing::info!("Signatures applied successfully");

        // Serialize the transaction
        // No transparent inputs
        tx_data.push(0x00); // vin count

        // Transparent outputs (one for the payment)
        tx_data.extend_from_slice(&serialize_compact_size(1));
        tx_data.extend_from_slice(&transparent_output);

        // No Sapling
        tx_data.push(0x00); // nSpendsSapling
        tx_data.push(0x00); // nOutputsSapling

        // Serialize Orchard bundle
        self.serialize_orchard_bundle(&authorized_bundle, tx_data)?;

        tracing::info!(
            "Built deshielding transaction: {} spends, 1 transparent output, {} shielded change, {} bytes",
            selected_notes_with_paths.len(),
            if change_amount > 0 { "with" } else { "no" },
            tx_data.len()
        );

        Ok(())
    }

    /// Build a transparent output (P2PKH) for deshielding
    fn build_transparent_output(&self, address: &str, value_zatoshis: u64) -> OrchardResult<Vec<u8>> {
        use sha2::{Digest, Sha256};

        let mut output = Vec::new();

        // Value (8 bytes, little-endian)
        output.extend_from_slice(&(value_zatoshis as i64).to_le_bytes());

        // Decode transparent address to get pubkey hash
        let decoded = bs58::decode(address)
            .into_vec()
            .map_err(|e| OrchardError::TransactionBuild(format!("Invalid transparent address: {}", e)))?;

        if decoded.len() < 26 {
            return Err(OrchardError::TransactionBuild(
                "Transparent address too short".to_string()
            ));
        }

        // Verify checksum
        let checksum = Sha256::digest(&Sha256::digest(&decoded[..22]));
        if &checksum[..4] != &decoded[22..26] {
            return Err(OrchardError::TransactionBuild(
                "Invalid transparent address checksum".to_string()
            ));
        }

        // Extract pubkey hash (bytes 2-22)
        let pubkey_hash = &decoded[2..22];

        // Build P2PKH script: OP_DUP OP_HASH160 <pubkey_hash> OP_EQUALVERIFY OP_CHECKSIG
        let script_pubkey = [
            0x76, // OP_DUP
            0xa9, // OP_HASH160
            0x14, // Push 20 bytes
        ]
        .iter()
        .chain(pubkey_hash.iter())
        .chain([
            0x88, // OP_EQUALVERIFY
            0xac, // OP_CHECKSIG
        ].iter())
        .copied()
        .collect::<Vec<u8>>();

        // Script length + script
        output.extend_from_slice(&serialize_compact_size(script_pubkey.len() as u64));
        output.extend_from_slice(&script_pubkey);

        tracing::debug!(
            "Built transparent output: {} zatoshis to {} (script: {})",
            value_zatoshis,
            address,
            hex::encode(&script_pubkey)
        );

        Ok(output)
    }

    /// Compute sighash for deshielding transaction from unauthorized bundle
    ///
    /// This must be called BEFORE creating the proof, using the unauthorized bundle.
    /// Following librustzcash pattern where sighash is computed from unauthed_tx.
    fn compute_deshielding_sighash_from_unauth(
        &self,
        transparent_output: &[u8],
        expiry_height: u32,
        consensus_branch_id: u32,
        bundle: &orchard::bundle::Bundle<InProgress<orchard::builder::Unproven, Unauthorized>, i64>,
    ) -> OrchardResult<[u8; 32]> {
        // T.1: header_digest
        let mut header_data = Vec::new();
        header_data.extend_from_slice(&TX_VERSION_WITH_OVERWINTERED.to_le_bytes());
        header_data.extend_from_slice(&VERSION_GROUP_ID_V5.to_le_bytes());
        header_data.extend_from_slice(&consensus_branch_id.to_le_bytes());
        header_data.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        header_data.extend_from_slice(&expiry_height.to_le_bytes());
        let header_digest = blake2b_256(b"ZTxIdHeadersHash", &header_data);

        // T.2: transparent_sig_digest for SignableInput::Shielded with no transparent inputs
        let prevouts_digest = blake2b_256(ZCASH_PREVOUTS_HASH, &[]);
        let sequence_digest = blake2b_256(ZCASH_SEQUENCE_HASH, &[]);
        let outputs_digest = blake2b_256(ZCASH_OUTPUTS_HASH, transparent_output);

        let mut transparent_data = Vec::new();
        transparent_data.extend_from_slice(&prevouts_digest);
        transparent_data.extend_from_slice(&sequence_digest);
        transparent_data.extend_from_slice(&outputs_digest);
        let transparent_sig_digest = blake2b_256(ZCASH_TRANSPARENT_HASH, &transparent_data);

        // T.3: sapling_digest (empty)
        let sapling_digest = blake2b_256(b"ZTxIdSaplingHash", &[]);

        // T.4: orchard_digest from unauthorized bundle
        // F4.0-a 旧 Orchard 池 tx = v5(VERSION_GROUP_ID_V5)。双池接缝:Ironwood 交易改 TxVersion::V6。
        let orchard_commitment = bundle.commitment(TxVersion::V5)
            .map_err(|e| OrchardError::TransactionBuild(format!("Orchard commitment failed: {:?}", e)))?;
        let orchard_digest: [u8; 32] = orchard_commitment.0.as_bytes().try_into()
            .map_err(|_| OrchardError::TransactionBuild("Invalid orchard commitment".to_string()))?;

        tracing::debug!("Deshielding sighash from unauth bundle:");
        tracing::debug!("  header_digest: {}", hex::encode(&header_digest));
        tracing::debug!("  transparent_sig_digest: {}", hex::encode(&transparent_sig_digest));
        tracing::debug!("  sapling_digest: {}", hex::encode(&sapling_digest));
        tracing::debug!("  orchard_digest: {}", hex::encode(&orchard_digest));

        // Final sighash
        let mut personalization = ZCASH_TX_HASH.to_vec();
        personalization.extend_from_slice(&consensus_branch_id.to_le_bytes());

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(&header_digest);
        sig_data.extend_from_slice(&transparent_sig_digest);
        sig_data.extend_from_slice(&sapling_digest);
        sig_data.extend_from_slice(&orchard_digest);

        Ok(blake2b_256(&personalization, &sig_data))
    }

    /// Compute sighash for deshielding transaction (SignableInput::Shielded with transparent outputs)
    ///
    /// According to ZIP 244:
    /// - When there are no transparent inputs (but there are transparent outputs),
    ///   transparent_sig_digest = hash_transparent_txid_data(txid_digests)
    /// - This is simpler than the case with transparent inputs
    fn compute_deshielding_sighash<V: Copy + Into<i64>>(
        &self,
        transparent_output: &[u8],
        expiry_height: u32,
        consensus_branch_id: u32,
        bundle: &orchard::bundle::Bundle<InProgress<Proof, Unauthorized>, V>,
    ) -> OrchardResult<[u8; 32]> {
        // T.1: header_digest
        let mut header_data = Vec::new();
        header_data.extend_from_slice(&TX_VERSION_WITH_OVERWINTERED.to_le_bytes());
        header_data.extend_from_slice(&VERSION_GROUP_ID_V5.to_le_bytes());
        header_data.extend_from_slice(&consensus_branch_id.to_le_bytes());
        header_data.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        header_data.extend_from_slice(&expiry_height.to_le_bytes());
        let header_digest = blake2b_256(b"ZTxIdHeadersHash", &header_data);

        // T.2: transparent_sig_digest for SignableInput::Shielded with no transparent inputs
        // According to ZIP 244 Section S.2:
        // When bundle.vin.is_empty(), use hash_transparent_txid_data(Some(txid_digests))
        // This hashes: prevouts_digest || sequence_digest || outputs_digest

        // prevouts_digest - empty since no inputs
        let prevouts_digest = blake2b_256(ZCASH_PREVOUTS_HASH, &[]);
        // sequence_digest - empty since no inputs
        let sequence_digest = blake2b_256(ZCASH_SEQUENCE_HASH, &[]);
        // outputs_digest - hash of transparent outputs
        let outputs_digest = blake2b_256(ZCASH_OUTPUTS_HASH, transparent_output);

        // hash_transparent_txid_data format: just concatenate the three digests
        let mut transparent_data = Vec::new();
        transparent_data.extend_from_slice(&prevouts_digest);
        transparent_data.extend_from_slice(&sequence_digest);
        transparent_data.extend_from_slice(&outputs_digest);
        let transparent_sig_digest = blake2b_256(ZCASH_TRANSPARENT_HASH, &transparent_data);

        // T.3: sapling_digest (empty)
        let sapling_digest = blake2b_256(b"ZTxIdSaplingHash", &[]);

        // T.4: orchard_digest
        let orchard_digest = compute_orchard_digest_from_proven(bundle);

        tracing::debug!("Deshielding sighash components:");
        tracing::debug!("  header_digest: {}", hex::encode(&header_digest));
        tracing::debug!("  prevouts_digest: {}", hex::encode(&prevouts_digest));
        tracing::debug!("  sequence_digest: {}", hex::encode(&sequence_digest));
        tracing::debug!("  outputs_digest: {}", hex::encode(&outputs_digest));
        tracing::debug!("  transparent_sig_digest: {}", hex::encode(&transparent_sig_digest));
        tracing::debug!("  sapling_digest: {}", hex::encode(&sapling_digest));
        tracing::debug!("  orchard_digest: {}", hex::encode(&orchard_digest));

        // Final sighash
        let mut personalization = ZCASH_TX_HASH.to_vec();
        personalization.extend_from_slice(&consensus_branch_id.to_le_bytes());

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(&header_digest);
        sig_data.extend_from_slice(&transparent_sig_digest);
        sig_data.extend_from_slice(&sapling_digest);
        sig_data.extend_from_slice(&orchard_digest);

        Ok(blake2b_256(&personalization, &sig_data))
    }

    /// Create merkle path for a note
    /// NOTE: This uses a simplified approach. For real chain validation,
    /// you need the actual merkle witness from the commitment tree.
    fn create_merkle_path_for_note(
        &self,
        note: &OrchardNote,
    ) -> OrchardResult<orchard::tree::MerklePath> {
        use orchard::tree::{MerkleHashOrchard, MerklePath};

        // For a real implementation, we would:
        // 1. Track the commitment tree incrementally
        // 2. Store the witness (merkle path) for each note
        // 3. Update witnesses as new blocks are added
        //
        // For now, we create a path that may not validate on-chain
        // but allows the code structure to be tested

        let position = note.position as u32;

        // Create empty leaf for the auth path
        let empty_leaf = MerkleHashOrchard::empty_leaf();

        // Create auth path from stored merkle_path if available, otherwise use empty
        let auth_path: [MerkleHashOrchard; ORCHARD_MERKLE_DEPTH] = if let Some(ref path) = note.merkle_path {
            if path.len() >= ORCHARD_MERKLE_DEPTH {
                let mut arr = [empty_leaf; ORCHARD_MERKLE_DEPTH];
                for (i, hash) in path.iter().take(ORCHARD_MERKLE_DEPTH).enumerate() {
                    // Convert [u8; 32] to MerkleHashOrchard via pallas::Base
                    let base_opt: subtle::CtOption<pallas::Base> = pallas::Base::from_repr(*hash);
                    if base_opt.is_some().into() {
                        arr[i] = MerkleHashOrchard::from_bytes(&hash)
                            .unwrap_or(empty_leaf);
                    }
                }
                arr
            } else {
                [empty_leaf; ORCHARD_MERKLE_DEPTH]
            }
        } else {
            // No merkle path stored - use empty leaves
            // This won't validate on chain but allows code testing
            tracing::warn!(
                "Note at position {} has no merkle path, using empty leaves. \
                 This transaction may not validate on-chain.",
                position
            );
            [empty_leaf; ORCHARD_MERKLE_DEPTH]
        };

        Ok(MerklePath::from_parts(position, auth_path))
    }

    /// Build transparent-to-shielded (shielding) bundle
    fn build_shielding_bundle(
        &self,
        tx_data: &mut Vec<u8>,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        private_key_hex: &str,
        transparent_inputs: Vec<TransparentInput>,
        _anchor: Anchor,  // Now uses orchard::tree::Anchor directly
    ) -> OrchardResult<()> {
        // Calculate total transparent input
        let total_transparent_input: u64 = transparent_inputs.iter().map(|i| i.value).sum();
        let num_inputs = transparent_inputs.len() as u64;

        // Recalculate fee based on actual input count (ZIP-317)
        // fee = 5000 * max(2, transparent_inputs + orchard_actions)
        // orchard_actions = 2 (payment + change, padded to even number)
        let orchard_actions: u64 = 2;
        let logical_actions = num_inputs + orchard_actions;
        let actual_fee = 5000 * std::cmp::max(2, logical_actions);

        tracing::info!(
            "build_shielding_bundle: {} inputs totaling {} zatoshis, amount={}, actual_fee={} (proposal_fee={})",
            num_inputs,
            total_transparent_input,
            proposal.amount_zatoshis,
            actual_fee,
            proposal.fee_zatoshis
        );

        // Use the higher of proposal fee or actual required fee
        let effective_fee = std::cmp::max(proposal.fee_zatoshis, actual_fee);

        // Step 1: Build the Orchard proven bundle (without binding signature yet)
        let proven_bundle = self.create_orchard_proven_bundle_with_fee(
            proposal,
            spending_key,
            total_transparent_input,
            effective_fee,
        )?;

        // Step 2: Compute the shielded sighash for binding signature
        // For shielding tx, we need: header_digest, transparent_txid_digest, sapling_digest, orchard_digest
        let shielded_sighash = self.compute_shielded_sighash(
            &transparent_inputs,
            proposal.expiry_height as u32,
            self.effective_branch_id(),
            &proven_bundle,
        )?;

        tracing::debug!("Shielded sighash for binding sig: {}", hex::encode(&shielded_sighash));

        // Step 3: Apply signatures with correct sighash
        let saks: &[SpendAuthorizingKey] = &[]; // No spend auth keys for output-only
        let orchard_bundle = proven_bundle
            .apply_signatures(OsRng, shielded_sighash, saks)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to apply signatures: {:?}", e)))?;

        // Step 4: Sign the transparent inputs using the authorized bundle
        let signed_inputs = sign_transparent_inputs_with_bundle(
            &transparent_inputs,
            private_key_hex,
            proposal.expiry_height as u32,
            self.effective_branch_id(),
            &orchard_bundle,
        )?;

        // Step 5: Write signed transparent inputs
        tx_data.extend_from_slice(&serialize_compact_size(signed_inputs.len() as u64));

        for input in &signed_inputs {
            // Previous output hash (32 bytes, little-endian)
            let mut txid_le = input.prev_tx_hash;
            txid_le.reverse();
            tx_data.extend_from_slice(&txid_le);
            // Previous output index (4 bytes)
            tx_data.extend_from_slice(&input.prev_tx_index.to_le_bytes());
            // Script sig length + script sig
            tx_data.extend_from_slice(&serialize_compact_size(input.script_sig.len() as u64));
            tx_data.extend_from_slice(&input.script_sig);
            // Sequence (4 bytes)
            tx_data.extend_from_slice(&input.sequence.to_le_bytes());
        }

        // No transparent outputs (all going to shielded)
        tx_data.push(0x00); // vout count

        // No Sapling spends
        tx_data.push(0x00);

        // No Sapling outputs
        tx_data.push(0x00);

        // Serialize and write the Orchard bundle
        let bundle_start = tx_data.len();
        self.serialize_orchard_bundle(&orchard_bundle, tx_data)?;
        let bundle_len = tx_data.len() - bundle_start;

        tracing::info!(
            "Built shielding transaction: {} transparent inputs, {} bytes Orchard bundle",
            signed_inputs.len(),
            bundle_len
        );

        Ok(())
    }

    /// Compute shielded sighash for binding signature (SignableInput::Shielded equivalent)
    /// This follows ZIP 244 for v5 transactions with SignableInput::Shielded
    fn compute_shielded_sighash<V: Copy + Into<i64>>(
        &self,
        transparent_inputs: &[TransparentInput],
        expiry_height: u32,
        consensus_branch_id: u32,
        bundle: &orchard::bundle::Bundle<InProgress<Proof, Unauthorized>, V>,
    ) -> OrchardResult<[u8; 32]> {
        // T.1: header_digest
        let mut header_data = Vec::new();
        header_data.extend_from_slice(&TX_VERSION_WITH_OVERWINTERED.to_le_bytes());
        header_data.extend_from_slice(&VERSION_GROUP_ID_V5.to_le_bytes());
        header_data.extend_from_slice(&consensus_branch_id.to_le_bytes());
        header_data.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        header_data.extend_from_slice(&expiry_height.to_le_bytes());
        let header_digest = blake2b_256(b"ZTxIdHeadersHash", &header_data);

        // T.2: transparent_sig_digest for SignableInput::Shielded
        // According to ZIP 244, this requires:
        // - hash_type (SIGHASH_ALL = 0x01)
        // - prevouts_digest
        // - amounts_digest (hash of all input amounts)
        // - scripts_digest (hash of all input scriptPubKeys)
        // - sequences_digest
        // - outputs_digest (empty for shielding)
        // - txin_sig_digest (empty for Shielded input type)
        let transparent_sig_digest = if transparent_inputs.is_empty() {
            // No transparent inputs - use empty hash
            blake2b_256(ZCASH_TRANSPARENT_HASH, &[])
        } else {
            let prevouts_digest = hash_prevouts(transparent_inputs);
            let amounts_digest = hash_amounts(transparent_inputs);
            let scripts_digest = hash_script_pubkeys(transparent_inputs);
            let sequences_digest = hash_sequences(transparent_inputs);
            // outputs_digest is empty since no transparent outputs
            let outputs_digest = blake2b_256(ZCASH_OUTPUTS_HASH, &[]);
            // txin_sig_digest is empty for SignableInput::Shielded
            let txin_sig_digest = blake2b_256(ZCASH_TRANSPARENT_SIG, &[]);

            let mut transparent_data = Vec::new();
            transparent_data.push(0x01); // SIGHASH_ALL
            transparent_data.extend_from_slice(&prevouts_digest);
            transparent_data.extend_from_slice(&amounts_digest);
            transparent_data.extend_from_slice(&scripts_digest);
            transparent_data.extend_from_slice(&sequences_digest);
            transparent_data.extend_from_slice(&outputs_digest);
            transparent_data.extend_from_slice(&txin_sig_digest);
            blake2b_256(ZCASH_TRANSPARENT_HASH, &transparent_data)
        };

        // T.3: sapling_digest (empty)
        let sapling_digest = blake2b_256(b"ZTxIdSaplingHash", &[]);

        // T.4: orchard_digest - use the bundle's commitment
        let orchard_digest = compute_orchard_digest_from_proven(bundle);

        tracing::debug!("Shielded sighash components:");
        tracing::debug!("  header_digest: {}", hex::encode(&header_digest));
        tracing::debug!("  transparent_sig_digest: {}", hex::encode(&transparent_sig_digest));
        tracing::debug!("  sapling_digest: {}", hex::encode(&sapling_digest));
        tracing::debug!("  orchard_digest: {}", hex::encode(&orchard_digest));

        // Final sighash
        let mut personalization = ZCASH_TX_HASH.to_vec();
        personalization.extend_from_slice(&consensus_branch_id.to_le_bytes());

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(&header_digest);
        sig_data.extend_from_slice(&transparent_sig_digest);
        sig_data.extend_from_slice(&sapling_digest);
        sig_data.extend_from_slice(&orchard_digest);

        Ok(blake2b_256(&personalization, &sig_data))
    }

    /// Create Orchard actions (spends + outputs)
    fn create_orchard_actions(
        &self,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        notes: Vec<OrchardNote>,
        anchor: [u8; 32],
    ) -> OrchardResult<Vec<u8>> {
        let mut bundle = Vec::new();

        // Select notes to spend
        let (selected_notes, total_input) = self.select_notes(
            notes,
            proposal.amount_zatoshis + proposal.fee_zatoshis,
        )?;

        let num_spends = selected_notes.len();
        let has_change = total_input > proposal.amount_zatoshis + proposal.fee_zatoshis;
        let num_outputs = if has_change { 2 } else { 1 }; // payment + optional change

        // Pad to even number (Orchard requirement)
        let num_actions = std::cmp::max(num_spends, num_outputs);
        let num_actions = if num_actions % 2 == 1 { num_actions + 1 } else { num_actions };

        // Number of actions
        bundle.push(num_actions as u8);

        // Create actions
        for i in 0..num_actions {
            let action = self.create_action(
                i,
                if i < num_spends { Some(&selected_notes[i]) } else { None },
                proposal,
                spending_key,
                i == 0, // First action is the payment
                has_change && i == 1, // Second action is change if needed
                total_input,
            )?;
            bundle.extend_from_slice(&action);
        }

        // Flags
        let flags = 0x03u8; // spends_enabled | outputs_enabled
        bundle.push(flags);

        // Value balance (negative means shielded pool gains value)
        let value_balance: i64 = 0; // For shielded-to-shielded, net is 0
        bundle.extend_from_slice(&value_balance.to_le_bytes());

        // Anchor
        bundle.extend_from_slice(&anchor);

        // Proof (placeholder - real implementation generates Halo 2 proofs)
        let proof_size = num_actions * 2720; // Each action proof is 2720 bytes
        bundle.extend_from_slice(&(proof_size as u32).to_le_bytes());
        bundle.extend_from_slice(&vec![0u8; proof_size]);

        // Binding signature (64 bytes)
        let binding_sig = self.create_binding_signature(spending_key)?;
        bundle.extend_from_slice(&binding_sig);

        Ok(bundle)
    }

    /// Create Orchard proven bundle (for shielding) - without binding signature
    /// Returns a proven bundle that needs apply_signatures with correct sighash
    ///
    /// # Arguments
    /// * `proposal` - Transfer proposal with amount and recipient
    /// * `spending_key` - Sender's spending key (for change address and OVK)
    /// * `total_transparent_input` - Total value of transparent inputs (for calculating change)
    /// * `effective_fee` - Actual fee to use (may differ from proposal.fee_zatoshis for ZIP-317 compliance)
    fn create_orchard_proven_bundle_with_fee(
        &self,
        proposal: &TransferProposal,
        spending_key: &OrchardSpendingKey,
        total_transparent_input: u64,
        effective_fee: u64,
    ) -> OrchardResult<orchard::bundle::Bundle<InProgress<Proof, Unauthorized>, i64>> {
        use super::address::OrchardAddressManager;
        use orchard::keys::Scope;

        // CRITICAL SAFETY CHECK: Prevent zero-value Orchard bundles
        if proposal.amount_zatoshis == 0 {
            tracing::error!(
                "BLOCKED in create_orchard_proven_bundle: amount_zatoshis=0! proposal={}, to={}",
                proposal.proposal_id,
                proposal.to_address
            );
            return Err(OrchardError::TransactionBuild(
                "Cannot create Orchard output with zero value - this would cause complete fund loss".to_string()
            ));
        }

        // Calculate change amount using effective fee
        let total_output = proposal.amount_zatoshis + effective_fee;
        if total_transparent_input < total_output {
            return Err(OrchardError::InsufficientBalance {
                available: total_transparent_input,
                required: total_output,
            });
        }
        let change_amount = total_transparent_input - total_output;

        tracing::info!(
            "Shielding transaction: input={}, amount={}, fee={}, change={}",
            total_transparent_input,
            proposal.amount_zatoshis,
            effective_fee,
            change_amount
        );

        // Get the proving key (cached globally)
        let pk = get_proving_key_for(OrchardCircuitVersion::FixedPostNu6_2);

        // For shielding (no spends), we use an empty tree anchor
        let anchor = Anchor::empty_tree();

        // Create builder with DEFAULT bundle type
        let bundle_type = BundleType::DEFAULT;
        // F4.0-a 旧 Orchard 池:orchard_v2()(NU6.2→NU6.3 语义)+ 其 default_flags。
        // 双池接缝:Ironwood 改 BundleVersion::ironwood_v3(),旧池 NU6.3 后改 orchard_v3()。
        let bundle_version = BundleVersion::orchard_v2();
        let bundle_flags = bundle_version.default_flags();
        let mut builder = OrchardBuilder::new(bundle_type, bundle_version, bundle_flags, anchor)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create builder: {:?}", e)))?;

        // Parse recipient address to get Orchard receiver
        let recipient_address = OrchardAddressManager::extract_orchard_address(&proposal.to_address)?;

        // Get OVK for sender to be able to decrypt outgoing transaction
        let ovk = Some(spending_key.to_ovk());

        // === Output 1: Payment to recipient ===
        tracing::info!(
            "Creating Orchard payment output: amount={} zatoshis, to={}...",
            proposal.amount_zatoshis,
            &proposal.to_address[..std::cmp::min(20, proposal.to_address.len())]
        );

        let payment_value = NoteValue::from_raw(proposal.amount_zatoshis);
        let memo_bytes: [u8; 512] = {
            let mut bytes = [0u8; 512];
            if let Some(m) = proposal.memo.as_ref() {
                let memo_data = m.as_bytes();
                let len = std::cmp::min(memo_data.len(), 512);
                bytes[..len].copy_from_slice(&memo_data[..len]);
            }
            bytes
        };

        builder
            .add_output(ovk.clone(), recipient_address, payment_value, memo_bytes)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to add payment output: {:?}", e)))?;

        // === Output 2: Change to sender (if any) ===
        if change_amount > 0 {
            // Get sender's own Orchard address for change
            let fvk = spending_key.to_fvk();
            // Use diversifier index 0 for change address
            let change_diversifier = orchard::keys::Diversifier::from_bytes([0u8; 11]);
            let change_address = fvk.address(change_diversifier, Scope::Internal);

            tracing::info!(
                "Creating Orchard change output: amount={} zatoshis (to self)",
                change_amount
            );

            let change_value = NoteValue::from_raw(change_amount);
            // No memo for change output (empty memo)
            builder
                .add_output(ovk, change_address, change_value, [0u8; 512])
                .map_err(|e| OrchardError::TransactionBuild(format!("Failed to add change output: {:?}", e)))?;
        }

        // Build the bundle (returns UnauthorizedBundle)
        let (unauthorized_bundle, _meta) = builder
            .build::<i64>(&mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to build bundle: {:?}", e)))?
            .ok_or_else(|| OrchardError::TransactionBuild("Empty bundle".to_string()))?;

        // Log the value_balance to debug
        let value_balance = *unauthorized_bundle.value_balance();

        // Expected value_balance = -(payment + change) = -(total_input - fee)
        let total_shielded_output = proposal.amount_zatoshis + change_amount;
        let expected_value_balance = -(total_shielded_output as i64);

        tracing::info!(
            "Orchard bundle built: value_balance={}, expected={}, actions={}, outputs={}",
            value_balance,
            expected_value_balance,
            unauthorized_bundle.actions().len(),
            if change_amount > 0 { 2 } else { 1 }
        );

        // CRITICAL SAFETY CHECK: Verify value_balance is correct
        if value_balance != expected_value_balance {
            tracing::error!(
                "CRITICAL: Orchard bundle value_balance mismatch! got={}, expected={}, payment={}, change={}",
                value_balance,
                expected_value_balance,
                proposal.amount_zatoshis,
                change_amount
            );
            if value_balance == 0 {
                return Err(OrchardError::TransactionBuild(
                    format!(
                        "CRITICAL: Orchard bundle has zero value_balance! This would cause complete fund loss. \
                         Expected: {}, Payment: {} zatoshis, Change: {} zatoshis",
                        expected_value_balance, proposal.amount_zatoshis, change_amount
                    )
                ));
            }
        }

        // Additional safety: verify the math
        // transparent_input = |value_balance| + fee
        // transparent_input = (payment + change) + fee
        let computed_input = ((-value_balance) as u64) + effective_fee;
        if computed_input != total_transparent_input {
            tracing::error!(
                "CRITICAL: Input/output mismatch! computed_input={}, actual_input={}, fee={}",
                computed_input,
                total_transparent_input,
                effective_fee
            );
            return Err(OrchardError::TransactionBuild(
                format!(
                    "Transaction balance mismatch: computed input {} != actual input {}",
                    computed_input, total_transparent_input
                )
            ));
        }

        tracing::info!(
            "Transaction balance verified: {} (input) = {} (shielded) + {} (fee)",
            total_transparent_input,
            total_shielded_output,
            effective_fee
        );

        // Create proof - returns a proven but not yet signed bundle
        let proven_bundle = unauthorized_bundle
            .create_proof(pk, &mut OsRng)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to create proof: {:?}", e)))?;

        Ok(proven_bundle)
    }

    /// Serialize an authorized Orchard bundle to bytes
    /// Uses zcash_primitives for correct serialization format
    fn serialize_orchard_bundle(
        &self,
        bundle: &orchard::Bundle<orchard::bundle::Authorized, i64>,
        output: &mut Vec<u8>,
    ) -> OrchardResult<()> {
        use zcash_primitives::transaction::components::orchard as orchard_serialization;
        use zcash_protocol::value::ZatBalance;

        // Convert Bundle<Authorized, i64> to Bundle<Authorized, ZatBalance>
        let value_balance_i64 = *bundle.value_balance();
        let zat_balance = ZatBalance::from_i64(value_balance_i64)
            .map_err(|_| OrchardError::TransactionBuild("Invalid value balance".to_string()))?;

        let bundle_with_zat = bundle.clone().try_map_value_balance::<ZatBalance, _, _>(|_| Ok(zat_balance))
            .map_err(|e: std::convert::Infallible| OrchardError::TransactionBuild(format!("Failed to map value balance: {:?}", e)))?;

        // Use zcash_primitives serialization
        let num_actions = bundle.actions().len();
        orchard_serialization::write_v5_bundle(Some(&bundle_with_zat), &mut *output)
            .map_err(|e| OrchardError::TransactionBuild(format!("Failed to serialize bundle: {}", e)))?;

        tracing::debug!(
            "Serialized Orchard bundle: {} actions, {} bytes total",
            num_actions,
            output.len()
        );

        Ok(())
    }

    /// Select notes to cover the required amount
    fn select_notes(
        &self,
        mut notes: Vec<OrchardNote>,
        amount_needed: u64,
    ) -> OrchardResult<(Vec<OrchardNote>, u64)> {
        if notes.is_empty() {
            return Err(OrchardError::NoSpendableNotes);
        }

        // Sort by value descending
        notes.sort_by(|a, b| b.value_zatoshis.cmp(&a.value_zatoshis));

        let mut selected = Vec::new();
        let mut total: u64 = 0;

        for note in notes {
            if total >= amount_needed {
                break;
            }
            total += note.value_zatoshis;
            selected.push(note);
        }

        if total < amount_needed {
            return Err(OrchardError::InsufficientBalance {
                available: total,
                required: amount_needed,
            });
        }

        Ok((selected, total))
    }

    /// Select notes with their MerklePaths to cover the required amount
    fn select_notes_with_paths(
        &self,
        mut notes_with_paths: Vec<(OrchardNote, MerklePath)>,
        amount_needed: u64,
    ) -> OrchardResult<(Vec<(OrchardNote, MerklePath)>, u64)> {
        if notes_with_paths.is_empty() {
            return Err(OrchardError::NoSpendableNotes);
        }

        // Sort by value descending
        notes_with_paths.sort_by(|a, b| b.0.value_zatoshis.cmp(&a.0.value_zatoshis));

        let mut selected = Vec::new();
        let mut total: u64 = 0;

        for (note, path) in notes_with_paths {
            if total >= amount_needed {
                break;
            }
            total += note.value_zatoshis;
            selected.push((note, path));
        }

        if total < amount_needed {
            return Err(OrchardError::InsufficientBalance {
                available: total,
                required: amount_needed,
            });
        }

        Ok((selected, total))
    }

    /// Create a single Orchard action
    fn create_action(
        &self,
        _index: usize,
        spend_note: Option<&OrchardNote>,
        proposal: &TransferProposal,
        _spending_key: &OrchardSpendingKey,
        is_payment: bool,
        is_change: bool,
        total_input: u64,
    ) -> OrchardResult<Vec<u8>> {
        let mut action = Vec::new();

        // Nullifier (32 bytes) - from spend note or dummy
        if let Some(note) = spend_note {
            action.extend_from_slice(&note.nullifier);
        } else {
            action.extend_from_slice(&[0u8; 32]);
        }

        // Randomized verification key (32 bytes)
        let mut rk = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rk);
        action.extend_from_slice(&rk);

        // Note commitment (32 bytes)
        let cmx = self.compute_note_commitment(
            if is_payment { proposal.amount_zatoshis }
            else if is_change { total_input - proposal.amount_zatoshis - proposal.fee_zatoshis }
            else { 0 },
        )?;
        action.extend_from_slice(&cmx);

        // Encrypted note (580 bytes)
        let enc_ciphertext = self.encrypt_note(
            proposal,
            is_payment,
            is_change,
            total_input,
        )?;
        action.extend_from_slice(&enc_ciphertext);

        // Ephemeral key (32 bytes)
        let mut epk = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut epk);
        action.extend_from_slice(&epk);

        // Out ciphertext (80 bytes)
        action.extend_from_slice(&[0u8; 80]);

        // cv (value commitment, 32 bytes)
        let cv = self.compute_value_commitment(
            if is_payment { proposal.amount_zatoshis }
            else if is_change { total_input - proposal.amount_zatoshis - proposal.fee_zatoshis }
            else { 0 },
        )?;
        action.extend_from_slice(&cv);

        // Authorization signature (64 bytes)
        action.extend_from_slice(&[0u8; 64]);

        Ok(action)
    }

    /// Create output-only action
    fn create_output_action(
        &self,
        proposal: &TransferProposal,
        _spending_key: &OrchardSpendingKey,
    ) -> OrchardResult<Vec<u8>> {
        let mut action = Vec::new();

        // Nullifier (dummy for output-only)
        action.extend_from_slice(&[0u8; 32]);

        // rk
        let mut rk = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rk);
        action.extend_from_slice(&rk);

        // cmx
        let cmx = self.compute_note_commitment(proposal.amount_zatoshis)?;
        action.extend_from_slice(&cmx);

        // enc_ciphertext
        let enc = self.encrypt_note(proposal, true, false, proposal.amount_zatoshis)?;
        action.extend_from_slice(&enc);

        // epk
        let mut epk = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut epk);
        action.extend_from_slice(&epk);

        // out_ciphertext
        action.extend_from_slice(&[0u8; 80]);

        // cv
        let cv = self.compute_value_commitment(proposal.amount_zatoshis)?;
        action.extend_from_slice(&cv);

        // auth sig (dummy for output)
        action.extend_from_slice(&[0u8; 64]);

        Ok(action)
    }

    /// Create dummy action for padding
    fn create_dummy_action(&self) -> OrchardResult<Vec<u8>> {
        let mut action = Vec::new();

        // All zeros for dummy action
        action.extend_from_slice(&[0u8; 32]); // nullifier
        action.extend_from_slice(&[0u8; 32]); // rk
        action.extend_from_slice(&[0u8; 32]); // cmx
        action.extend_from_slice(&[0u8; 580]); // enc_ciphertext
        action.extend_from_slice(&[0u8; 32]); // epk
        action.extend_from_slice(&[0u8; 80]); // out_ciphertext
        action.extend_from_slice(&[0u8; 32]); // cv
        action.extend_from_slice(&[0u8; 64]); // auth sig

        Ok(action)
    }

    /// Compute note commitment
    fn compute_note_commitment(&self, value: u64) -> OrchardResult<[u8; 32]> {
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashOrchardCm__")
            .to_state();
        hasher.update(&value.to_le_bytes());

        let mut rcm = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rcm);
        hasher.update(&rcm);

        let result = hasher.finalize();
        let mut cm = [0u8; 32];
        cm.copy_from_slice(result.as_bytes());
        Ok(cm)
    }

    /// Compute value commitment
    fn compute_value_commitment(&self, value: u64) -> OrchardResult<[u8; 32]> {
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashOrchardCV__")
            .to_state();
        hasher.update(&value.to_le_bytes());

        let mut rcv = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rcv);
        hasher.update(&rcv);

        let result = hasher.finalize();
        let mut cv = [0u8; 32];
        cv.copy_from_slice(result.as_bytes());
        Ok(cv)
    }

    /// Encrypt note data
    fn encrypt_note(
        &self,
        proposal: &TransferProposal,
        is_payment: bool,
        is_change: bool,
        total_input: u64,
    ) -> OrchardResult<Vec<u8>> {
        let mut plaintext = Vec::new();

        // Note plaintext structure:
        // - 1 byte: lead byte (0x02 for Orchard)
        // - 11 bytes: diversifier
        // - 8 bytes: value
        // - 32 bytes: rseed
        // - 512 bytes: memo

        plaintext.push(0x02); // Orchard lead byte
        plaintext.extend_from_slice(&[0u8; 11]); // diversifier (would be derived from address)

        let value = if is_payment {
            proposal.amount_zatoshis
        } else if is_change {
            total_input - proposal.amount_zatoshis - proposal.fee_zatoshis
        } else {
            0
        };
        plaintext.extend_from_slice(&value.to_le_bytes());

        // Random rseed
        let mut rseed = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rseed);
        plaintext.extend_from_slice(&rseed);

        // Memo (512 bytes)
        let mut memo = [0u8; 512];
        if let Some(ref m) = proposal.memo {
            let bytes = m.as_bytes();
            let len = std::cmp::min(bytes.len(), 512);
            memo[..len].copy_from_slice(&bytes[..len]);
        }
        plaintext.extend_from_slice(&memo);

        // In real implementation, encrypt with recipient's key using ChaCha20Poly1305
        // For now, just pad to 580 bytes (encrypted size with tag)
        let mut ciphertext = plaintext;
        ciphertext.resize(580, 0);

        Ok(ciphertext)
    }

    /// Create binding signature
    fn create_binding_signature(&self, _spending_key: &OrchardSpendingKey) -> OrchardResult<[u8; 64]> {
        // In real implementation, this would sign with the binding key
        // For now, return placeholder
        let mut sig = [0u8; 64];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut sig);
        Ok(sig)
    }

    /// Compute transaction ID from raw transaction data
    fn compute_tx_id(&self, tx_data: &[u8]) -> String {
        let hash = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashTxId_____")
            .hash(tx_data);

        // Reverse for display (big-endian)
        let mut tx_id = hash.as_bytes().to_vec();
        tx_id.reverse();

        hex::encode(tx_id)
    }
}

/// Transparent input for shielding
#[derive(Debug, Clone)]
pub struct TransparentInput {
    /// Previous transaction hash (stored in big-endian for display, reversed for wire format)
    pub prev_tx_hash: [u8; 32],
    /// Previous output index
    pub prev_tx_index: u32,
    /// Script pubkey (the UTXO's locking script - NOT the signature)
    pub script_pubkey: Vec<u8>,
    /// Value in zatoshis
    pub value: u64,
    /// Sequence number
    pub sequence: u32,
}

/// Signed transparent input ready for serialization
#[derive(Debug, Clone)]
pub struct SignedTransparentInput {
    /// Previous transaction hash
    pub prev_tx_hash: [u8; 32],
    /// Previous output index
    pub prev_tx_index: u32,
    /// Signed script sig (signature + pubkey)
    pub script_sig: Vec<u8>,
    /// Sequence number
    pub sequence: u32,
}

// ============================================================================
// Transparent Input Signing Helpers (ZIP 244 compliant)
// ============================================================================

/// BLAKE2b-256 hash with personalization (ZIP 244 compliant)
fn blake2b_256(personalization: &[u8], data: &[u8]) -> [u8; 32] {
    let mut pers = [0u8; 16];
    let len = std::cmp::min(personalization.len(), 16);
    pers[..len].copy_from_slice(&personalization[..len]);

    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(&pers)
        .hash(data);

    let mut output = [0u8; 32];
    output.copy_from_slice(hash.as_bytes());
    output
}

/// Serialize a compact size integer (Bitcoin/Zcash varint)
fn serialize_compact_size(n: u64) -> Vec<u8> {
    if n < 0xfd {
        vec![n as u8]
    } else if n <= 0xffff {
        let mut v = vec![0xfd];
        v.extend_from_slice(&(n as u16).to_le_bytes());
        v
    } else if n <= 0xffffffff {
        let mut v = vec![0xfe];
        v.extend_from_slice(&(n as u32).to_le_bytes());
        v
    } else {
        let mut v = vec![0xff];
        v.extend_from_slice(&n.to_le_bytes());
        v
    }
}

/// Hash of all prevouts (ZIP 244)
fn hash_prevouts(inputs: &[TransparentInput]) -> [u8; 32] {
    let mut data = Vec::new();
    for input in inputs {
        // txid is stored big-endian, but serialized little-endian
        let mut txid_le = input.prev_tx_hash;
        txid_le.reverse();
        data.extend_from_slice(&txid_le);
        data.extend_from_slice(&input.prev_tx_index.to_le_bytes());
    }
    blake2b_256(ZCASH_PREVOUTS_HASH, &data)
}

/// Hash of all sequences (ZIP 244)
fn hash_sequences(inputs: &[TransparentInput]) -> [u8; 32] {
    let mut data = Vec::new();
    for input in inputs {
        data.extend_from_slice(&input.sequence.to_le_bytes());
    }
    blake2b_256(ZCASH_SEQUENCE_HASH, &data)
}

/// Hash of input amounts (ZIP 244)
fn hash_amounts(inputs: &[TransparentInput]) -> [u8; 32] {
    let mut data = Vec::new();
    for input in inputs {
        data.extend_from_slice(&(input.value as i64).to_le_bytes());
    }
    blake2b_256(b"ZTxTrAmountsHash", &data)
}

/// Hash of input scripts (ZIP 244)
fn hash_script_pubkeys(inputs: &[TransparentInput]) -> [u8; 32] {
    let mut data = Vec::new();
    for input in inputs {
        data.extend_from_slice(&serialize_compact_size(input.script_pubkey.len() as u64));
        data.extend_from_slice(&input.script_pubkey);
    }
    blake2b_256(b"ZTxTrScriptsHash", &data)
}

/// Calculate ZIP 244 sighash for a transparent input in a shielding transaction
fn calculate_shielding_sighash(
    inputs: &[TransparentInput],
    input_index: usize,
    expiry_height: u32,
    consensus_branch_id: u32,
    bundle: &orchard::Bundle<orchard::bundle::Authorized, i64>,
) -> OrchardResult<[u8; 32]> {
    let input = &inputs[input_index];

    tracing::debug!(
        "Calculating sighash for input {}: txid={} vout={} value={} script_len={}",
        input_index,
        hex::encode(&input.prev_tx_hash),
        input.prev_tx_index,
        input.value,
        input.script_pubkey.len()
    );
    tracing::debug!("Script pubkey: {}", hex::encode(&input.script_pubkey));

    // Build prevouts_sig_digest
    let prevouts_digest = hash_prevouts(inputs);
    tracing::debug!("prevouts_digest: {}", hex::encode(&prevouts_digest));

    // Build amounts_sig_digest
    let amounts_digest = hash_amounts(inputs);
    tracing::debug!("amounts_digest: {}", hex::encode(&amounts_digest));

    // Build scripts_sig_digest
    let scripts_digest = hash_script_pubkeys(inputs);
    tracing::debug!("scripts_digest: {}", hex::encode(&scripts_digest));

    // Build sequences_sig_digest
    let sequences_digest = hash_sequences(inputs);
    tracing::debug!("sequences_digest: {}", hex::encode(&sequences_digest));

    // Build outputs_sig_digest (empty for shielding - no transparent outputs)
    let outputs_digest = blake2b_256(ZCASH_OUTPUTS_HASH, &[]);
    tracing::debug!("outputs_digest: {}", hex::encode(&outputs_digest));

    // Build txin_sig_digest for this input
    let mut txin_data = Vec::new();
    let mut txid_le = input.prev_tx_hash;
    txid_le.reverse();
    txin_data.extend_from_slice(&txid_le);
    txin_data.extend_from_slice(&input.prev_tx_index.to_le_bytes());
    txin_data.extend_from_slice(&(input.value as i64).to_le_bytes());
    txin_data.extend_from_slice(&serialize_compact_size(input.script_pubkey.len() as u64));
    txin_data.extend_from_slice(&input.script_pubkey);
    txin_data.extend_from_slice(&input.sequence.to_le_bytes());

    tracing::debug!("txin_data ({} bytes): {}", txin_data.len(), hex::encode(&txin_data));

    let txin_sig_digest = blake2b_256(ZCASH_TRANSPARENT_SIG, &txin_data);
    tracing::debug!("txin_sig_digest: {}", hex::encode(&txin_sig_digest));

    // Combine for transparent_sig_digest
    let mut transparent_data = Vec::new();
    transparent_data.push(0x01); // SIGHASH_ALL
    transparent_data.extend_from_slice(&prevouts_digest);
    transparent_data.extend_from_slice(&amounts_digest);
    transparent_data.extend_from_slice(&scripts_digest);
    transparent_data.extend_from_slice(&sequences_digest);
    transparent_data.extend_from_slice(&outputs_digest);
    transparent_data.extend_from_slice(&txin_sig_digest);

    let transparent_sig_digest = blake2b_256(ZCASH_TRANSPARENT_HASH, &transparent_data);
    tracing::debug!("transparent_sig_digest: {}", hex::encode(&transparent_sig_digest));

    // Build header_digest
    let mut header_data = Vec::new();
    header_data.extend_from_slice(&TX_VERSION_WITH_OVERWINTERED.to_le_bytes());
    header_data.extend_from_slice(&VERSION_GROUP_ID_V5.to_le_bytes());
    header_data.extend_from_slice(&consensus_branch_id.to_le_bytes());
    header_data.extend_from_slice(&0u32.to_le_bytes()); // lock_time
    header_data.extend_from_slice(&expiry_height.to_le_bytes());

    tracing::debug!(
        "header_data: version={:08x} vg_id={:08x} branch_id={:08x} expiry={}",
        TX_VERSION_WITH_OVERWINTERED,
        VERSION_GROUP_ID_V5,
        consensus_branch_id,
        expiry_height
    );

    let header_digest = blake2b_256(b"ZTxIdHeadersHash", &header_data);
    tracing::debug!("header_digest: {}", hex::encode(&header_digest));

    // Empty sapling digest
    let sapling_digest = blake2b_256(b"ZTxIdSaplingHash", &[]);
    tracing::debug!("sapling_digest: {}", hex::encode(&sapling_digest));

    // Compute orchard_digest according to ZIP 244 T.4
    let orchard_digest = compute_orchard_digest(bundle);
    tracing::debug!("orchard_digest: {}", hex::encode(&orchard_digest));

    // Build the final sighash
    let mut personalization = ZCASH_TX_HASH.to_vec();
    personalization.extend_from_slice(&consensus_branch_id.to_le_bytes());

    let mut sig_data = Vec::new();
    sig_data.extend_from_slice(&header_digest);
    sig_data.extend_from_slice(&transparent_sig_digest);
    sig_data.extend_from_slice(&sapling_digest);
    sig_data.extend_from_slice(&orchard_digest);

    let sighash = blake2b_256(&personalization, &sig_data);
    tracing::info!("Final sighash: {}", hex::encode(&sighash));

    Ok(sighash)
}

/// Compute orchard_digest according to ZIP 244 T.4
/// Uses the orchard crate's built-in commitment() method for correctness
fn compute_orchard_digest(bundle: &orchard::Bundle<orchard::bundle::Authorized, i64>) -> [u8; 32] {
    // The orchard crate's commitment() method correctly implements ZIP 244 T.4
    // F4.0-a 旧 Orchard 池 + V5:commitments.rs 保证此组合永不 error(仅 Ironwood+V5 才 error)。
    let commitment = bundle.commitment(TxVersion::V5)
        .expect("Orchard pool V5 commitment is always valid");
    // BundleCommitment wraps a Blake2bHash, get the raw bytes
    let hash_bytes = commitment.0.as_bytes();
    let mut result = [0u8; 32];
    result.copy_from_slice(hash_bytes);
    result
}

/// Compute orchard_digest from a proven (but not yet authorized) bundle
/// Uses the bundle's built-in commitment() method for correctness
/// Used for computing the shielded sighash before apply_signatures
fn compute_orchard_digest_from_proven<V: Copy + Into<i64>>(
    bundle: &orchard::bundle::Bundle<InProgress<Proof, Unauthorized>, V>,
) -> [u8; 32] {
    // Use the bundle's built-in commitment() method which correctly implements ZIP 244 T.4
    // The InProgress bundle type implements Authorization trait, so commitment() is available
    // F4.0-a 旧 Orchard 池 + V5:commitments.rs 保证此组合永不 error(仅 Ironwood+V5 才 error)。
    let commitment = bundle.commitment(TxVersion::V5)
        .expect("Orchard pool V5 commitment is always valid");
    let hash_bytes = commitment.0.as_bytes();
    let mut result = [0u8; 32];
    result.copy_from_slice(hash_bytes);
    result
}

/// Sign a transparent input
fn sign_transparent_input(
    secp: &Secp256k1<secp256k1::All>,
    secret_key: &SecretKey,
    sighash: &[u8; 32],
) -> OrchardResult<Vec<u8>> {
    let message = Message::from_digest_slice(sighash)
        .map_err(|e| OrchardError::TransactionBuild(format!("Invalid sighash: {}", e)))?;

    let signature = secp.sign_ecdsa(&message, secret_key);

    // DER encode and append sighash type
    let mut sig_bytes = signature.serialize_der().to_vec();
    sig_bytes.push(0x01); // SIGHASH_ALL

    Ok(sig_bytes)
}

/// Build scriptSig for P2PKH input
fn build_p2pkh_scriptsig(signature: &[u8], public_key: &PublicKey) -> Vec<u8> {
    let pubkey_bytes = public_key.serialize(); // Compressed

    let mut script_sig = Vec::new();
    // Push signature
    script_sig.push(signature.len() as u8);
    script_sig.extend_from_slice(signature);
    // Push public key
    script_sig.push(pubkey_bytes.len() as u8);
    script_sig.extend_from_slice(&pubkey_bytes);

    script_sig
}

/// Sign all transparent inputs for a shielding transaction
pub fn sign_transparent_inputs_with_bundle(
    inputs: &[TransparentInput],
    private_key_hex: &str,
    expiry_height: u32,
    consensus_branch_id: u32,
    bundle: &orchard::Bundle<orchard::bundle::Authorized, i64>,
) -> OrchardResult<Vec<SignedTransparentInput>> {
    let secp = Secp256k1::new();

    // Parse private key
    let key_hex = private_key_hex.strip_prefix("0x").unwrap_or(private_key_hex);
    let key_bytes = hex::decode(key_hex)
        .map_err(|e| OrchardError::KeyDerivation(format!("Invalid private key hex: {}", e)))?;

    if key_bytes.len() != 32 {
        return Err(OrchardError::KeyDerivation(
            "Private key must be 32 bytes".to_string(),
        ));
    }

    let secret_key = SecretKey::from_slice(&key_bytes)
        .map_err(|e| OrchardError::KeyDerivation(format!("Invalid private key: {}", e)))?;
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    // Log pubkey info for debugging
    let pubkey_compressed = public_key.serialize();
    tracing::debug!("Signing with pubkey (compressed): {}", hex::encode(&pubkey_compressed));

    // Compute expected pubkey hash (for P2PKH verification)
    use ripemd::Ripemd160;
    use sha2::{Digest, Sha256};
    let sha256_hash = Sha256::digest(&pubkey_compressed);
    let pubkey_hash = Ripemd160::digest(&sha256_hash);
    tracing::debug!("Expected pubkey_hash (HASH160): {}", hex::encode(&pubkey_hash));

    // Sign each input
    let mut signed_inputs = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        // Calculate sighash with proper orchard_digest
        let sighash = calculate_shielding_sighash(
            inputs,
            i,
            expiry_height,
            consensus_branch_id,
            bundle,
        )?;

        // Sign
        let signature = sign_transparent_input(&secp, &secret_key, &sighash)?;
        tracing::debug!("Input {} signature ({} bytes): {}", i, signature.len(), hex::encode(&signature));

        // Build scriptSig
        let script_sig = build_p2pkh_scriptsig(&signature, &public_key);
        tracing::debug!("Input {} scriptSig ({} bytes): {}", i, script_sig.len(), hex::encode(&script_sig));

        signed_inputs.push(SignedTransparentInput {
            prev_tx_hash: input.prev_tx_hash,
            prev_tx_index: input.prev_tx_index,
            script_sig,
            sequence: input.sequence,
        });
    }

    tracing::info!("Signed {} transparent inputs for shielding", signed_inputs.len());
    Ok(signed_inputs)
}

/// Check if an address is a Zcash transparent address (t1... or t3...)
pub fn is_transparent_address(address: &str) -> bool {
    // Zcash mainnet transparent addresses start with t1 (P2PKH) or t3 (P2SH)
    // Length should be 34-35 characters for base58check encoded address
    (address.starts_with("t1") || address.starts_with("t3"))
        && address.len() >= 34
        && address.len() <= 36
        && address.chars().all(|c| {
            // Base58 characters (no 0, O, I, l)
            c.is_alphanumeric() && c != '0' && c != 'O' && c != 'I' && c != 'l'
        })
}

/// Check if an address is a Zcash unified address (u1...)
pub fn is_unified_address(address: &str) -> bool {
    address.starts_with("u1") && address.len() >= 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_request_zatoshis() {
        let request = TransferRequest {
            wallet_id: 1,
            to_address: "u1test".to_string(),
            amount_zec: "1.5".to_string(),
            amount_zatoshis: None,
            memo: None,
            fund_source: FundSource::Auto,
        };

        let zatoshis = request.get_zatoshis().unwrap();
        assert_eq!(zatoshis, 150_000_000);
    }

    #[test]
    fn test_fee_calculation() {
        let service = OrchardTransferService::new(NetworkType::Mainnet);

        // Minimum fee for 1 output
        let fee = service.calculate_fee(1, FundSource::Shielded);
        assert_eq!(fee, DEFAULT_FEE_ZATOSHIS);

        // Fee for multiple outputs
        let fee = service.calculate_fee(5, FundSource::Shielded);
        assert!(fee > DEFAULT_FEE_ZATOSHIS);
    }

    #[test]
    fn test_create_proposal() {
        let service = OrchardTransferService::new(NetworkType::Mainnet);

        let request = TransferRequest {
            wallet_id: 1,
            to_address: "u1test".to_string(),
            amount_zec: "0.001".to_string(),
            amount_zatoshis: None,
            memo: Some("Test memo".to_string()),
            fund_source: FundSource::Transparent,
        };

        let proposal = service.create_proposal(
            &request,
            1_000_000, // 0.01 ZEC transparent
            None,
            2_500_000,
        ).unwrap();

        assert_eq!(proposal.amount_zatoshis, 100_000);
        assert!(proposal.is_shielding);
        assert_eq!(proposal.fund_source, FundSource::Transparent);
    }
}
