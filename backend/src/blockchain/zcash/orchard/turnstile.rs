//! Post-NU6.3 v6 shielded transaction construction:
//!
//! * [`build_turnstile_transaction`] — old Orchard pool → Ironwood (the turnstile
//!   crossing that a migration is made of);
//! * [`build_ironwood_transaction`] — Ironwood → Ironwood (how migrated funds are
//!   spent, and after a completed migration the only way to spend at all);
//! * [`build_deshield_transaction`] — either shielded pool → a transparent
//!   address (paying out to a t-address).
//!
//! All three go through `zcash_primitives`' [`Builder`] and share this module's
//! consensus parameters, fail-loud Sapling prover and output shapes.
//!
//! After NU6.3 activates, spending an old Orchard-pool note crosses the
//! "turnstile" into the Ironwood pool: a v6 transaction carrying an
//! `orchard_v3` spend bundle (old pool, value out) and an `ironwood_v3` output
//! bundle (new pool, value in), balanced through the per-pool value balances.
//!
//! We build this via `zcash_primitives`' [`Builder`] (which handles v6
//! serialization, sighash_v6, proofs, and the cross-pool value balance) rather
//! than hand-rolling v6 — a hand-rolled sighash/serialization slip on a
//! consensus-critical path means stuck or lost funds.
//!
//! **Consensus shape (verified against `orchard` 0.15 / `zcash_primitives` 0.29
//! source):** post-NU6.3 the old Orchard pool (`orchard_v3`) mandates the
//! *cross-address* restriction — `permits_cross_address_transfers()` is `false`
//! only for `(V3, Orchard)` (see `orchard::bundle`; `default_flags(orchard_v3)`
//! is spends✓ outputs✓ cross_address✗). Precisely: an old-pool output to a
//! *different* address is consensus-invalid, while a *same-address* change
//! output would still be legal — the restriction is on cross-address transfers,
//! not on outputs wholesale. This builder nonetheless creates **no old-pool
//! output at all**: it adds only old-pool *spends* (their value leaves through
//! the bundle's positive value balance) and routes **all** value — both the
//! payment and any change — into the Ironwood pool via
//! [`Builder::add_ironwood_output`]. That is the migration semantics (funds
//! cross to the new pool) and it also cleanly sidesteps the cross-address
//! restriction, rather than depending on old-pool same-address change.
//!
//! The bundle versions (`orchard_v3` / `ironwood_v3`) and transaction version
//! (v6) are **not** chosen here: [`Builder::new`] derives them from the target
//! height via [`ZpayNetworkParams`] (the NU6.3 activation height read live from
//! the node), so a height below activation would silently fall back to the old
//! path. This module must therefore only be engaged at
//! `ProtocolEra::PostNu63`; the pre-NU6.3 Orchard-pool path in `transfer.rs` is
//! left completely untouched (it is live-verified and zero-regression).
//! See the F4.0-c P1 plan.

#![allow(dead_code)] // wired in as the turnstile build path lands piece by piece

use core::convert::Infallible;

use orchard::keys::{FullViewingKey, OutgoingViewingKey, SpendAuthorizingKey};
use orchard::tree::{Anchor, MerklePath};
use orchard::{Address, Note};
use rand::rngs::OsRng;
use rand::RngCore;
use zcash_primitives::transaction::builder::{BuildConfig, Builder};
use zcash_primitives::transaction::fees::fixed::FeeRule;
use zcash_protocol::consensus::{BlockHeight, NetworkType, NetworkUpgrade, Parameters};
use zcash_protocol::memo::MemoBytes;
use zcash_protocol::value::Zatoshis;
use zcash_transparent::builder::TransparentSigningSet;

use sha2::{Digest, Sha256};
use zcash_transparent::address::TransparentAddress;

use sapling::bundle::GrothProofBytes;
use sapling::prover::{OutputProver, SpendProver};

use super::ShieldedPool;

/// Consensus parameters sourced from the connected node's `getblockchaininfo`
/// `upgrades` table, so activation heights match whatever chain the RPC endpoint
/// points at — mainnet (NU6.3 = 3,428,143), testnet, or a regtest with a custom
/// NU6.3 (e.g. 档B @ 500). `zcash_primitives`' Builder needs a [`Parameters`] to
/// select the bundle version / branch for the target height, so we mirror the
/// node's truth rather than hardcode a network.
#[derive(Clone, Debug)]
pub struct ZpayNetworkParams {
    network_type: NetworkType,
    /// (upgrade, activation_height) pairs as reported by the node. `NetworkUpgrade`
    /// is not `Hash`, so a small Vec + linear scan (≤10 entries) is used.
    heights: Vec<(NetworkUpgrade, u64)>,
}

impl ZpayNetworkParams {
    /// Build params from a network type and the node's (upgrade, height) list.
    pub fn new(network_type: NetworkType, heights: Vec<(NetworkUpgrade, u64)>) -> Self {
        Self {
            network_type,
            heights,
        }
    }

    /// Map a node `getblockchaininfo.upgrades` name to a [`NetworkUpgrade`].
    /// Node monikers vary in punctuation/casing; NU6.3 may also be reported by
    /// its pool name "Ironwood" (mirrors the client's NU6.3 detection). Returns
    /// `None` for names we don't model.
    pub fn upgrade_from_name(name: &str) -> Option<NetworkUpgrade> {
        match name.to_ascii_lowercase().as_str() {
            "overwinter" => Some(NetworkUpgrade::Overwinter),
            "sapling" => Some(NetworkUpgrade::Sapling),
            "blossom" => Some(NetworkUpgrade::Blossom),
            "heartwood" => Some(NetworkUpgrade::Heartwood),
            "canopy" => Some(NetworkUpgrade::Canopy),
            "nu5" => Some(NetworkUpgrade::Nu5),
            "nu6" => Some(NetworkUpgrade::Nu6),
            "nu6.1" | "nu6_1" => Some(NetworkUpgrade::Nu6_1),
            "nu6.2" | "nu6_2" => Some(NetworkUpgrade::Nu6_2),
            "nu6.3" | "nu6_3" | "nu63" | "ironwood" => Some(NetworkUpgrade::Nu6_3),
            _ => None,
        }
    }
}

impl Parameters for ZpayNetworkParams {
    fn network_type(&self) -> NetworkType {
        self.network_type
    }

    fn activation_height(&self, nu: NetworkUpgrade) -> Option<BlockHeight> {
        self.heights
            .iter()
            .find(|(u, _)| *u == nu)
            .map(|(_, h)| block_height_saturating(*h))
    }
}

/// Zcash block heights are `u32` on the wire; node RPC hands us `u64`. Heights
/// live far below `u32::MAX` (mainnet NU6.3 = 3,428,143), so this only guards
/// against a malformed/huge value rather than a real height. Saturate (not
/// truncate) so a bogus value can never wrap to a small, *active* height: it
/// pins to `u32::MAX`, an unreachable height, so the turnstile era gate stays
/// closed. We deliberately do not `assert!`/`debug_assert!` here — zpay runs a
/// debug build in production (project rule), so an assert would panic the live
/// service on malformed node data instead of degrading safely.
fn block_height_saturating(h: u64) -> BlockHeight {
    BlockHeight::from_u32(h.min(u32::MAX as u64) as u32)
}

/// A Sapling prover that refuses to prove anything — **fail-loud**, not a mock.
///
/// The turnstile transaction carries no Sapling bundle, so the generic
/// `SpendProver` / `OutputProver` required by [`Builder::build`]'s signature are
/// structurally necessary but never invoked. Supplying this placeholder avoids
/// loading the multi-megabyte Sapling Groth parameters for a path that never
/// runs.
///
/// Every method panics with a clear message. "Never invoked" is a property of
/// today's call graph, not a permanent guarantee: if a future change ever routes
/// a Sapling spend/output through this builder, we want an immediate, obvious
/// crash — never a silently bogus proof. A dumb/silent mock here would violate
/// the no-mock rule; a fail-loud placeholder is its correct boundary.
struct FailLoudSaplingProver;

const FAIL_LOUD_MSG: &str =
    "FailLoudSaplingProver invoked: the Ironwood turnstile path must never build a \
     Sapling bundle. A Sapling spend/output reached the turnstile builder — this is \
     a bug. Refusing to produce a proof.";

impl SpendProver for FailLoudSaplingProver {
    type Proof = GrothProofBytes;

    fn prepare_circuit(
        _proof_generation_key: sapling::ProofGenerationKey,
        _diversifier: sapling::Diversifier,
        _rseed: sapling::Rseed,
        _value: sapling::value::NoteValue,
        _alpha: jubjub::Fr,
        _rcv: sapling::value::ValueCommitTrapdoor,
        _anchor: bls12_381::Scalar,
        _merkle_path: sapling::MerklePath,
    ) -> Option<sapling::circuit::Spend> {
        panic!("{FAIL_LOUD_MSG}");
    }

    fn create_proof<R: RngCore>(
        &self,
        _circuit: sapling::circuit::Spend,
        _rng: &mut R,
    ) -> Self::Proof {
        panic!("{FAIL_LOUD_MSG}");
    }

    fn encode_proof(_proof: Self::Proof) -> GrothProofBytes {
        panic!("{FAIL_LOUD_MSG}");
    }
}

impl OutputProver for FailLoudSaplingProver {
    type Proof = GrothProofBytes;

    fn prepare_circuit(
        _esk: &sapling::keys::EphemeralSecretKey,
        _payment_address: sapling::PaymentAddress,
        _rcm: jubjub::Fr,
        _value: sapling::value::NoteValue,
        _rcv: sapling::value::ValueCommitTrapdoor,
    ) -> sapling::circuit::Output {
        panic!("{FAIL_LOUD_MSG}");
    }

    fn create_proof<R: RngCore>(
        &self,
        _circuit: sapling::circuit::Output,
        _rng: &mut R,
    ) -> Self::Proof {
        panic!("{FAIL_LOUD_MSG}");
    }

    fn encode_proof(_proof: Self::Proof) -> GrothProofBytes {
        panic!("{FAIL_LOUD_MSG}");
    }
}

/// An Ironwood-pool output: the destination of migrated/turnstiled value.
///
/// Every unit of value spent from the old pool re-enters through these — the
/// payment to the recipient plus any change back to the sender's own (internal)
/// Ironwood address. There are deliberately no old-pool outputs (see module
/// docs).
pub struct TurnstileOutput {
    /// Outgoing viewing key so the sender can later detect/recover this output.
    /// `None` produces an output only the recipient can decrypt.
    pub ovk: Option<OutgoingViewingKey>,
    /// Recipient Orchard address (Ironwood outputs produce V3 notes; the address
    /// format is unchanged — the note version is a property of the pool).
    pub recipient: Address,
    pub value_zatoshis: u64,
    pub memo: MemoBytes,
}

/// Change retained in the **old Orchard pool** instead of crossing to Ironwood.
///
/// Used by intermediate batches of a private (staggered) migration: if every
/// batch pushed its change through the turnstile, the first batch would empty
/// the old pool and there would be nothing left for the later batches to
/// migrate. Keeping change on the old side preserves the schedule.
///
/// Post-NU6.3 the old pool disables cross-address transfers, so a plain output
/// is consensus-invalid; the only legal way to retain value is a
/// wallet-controlled change output, which the builder pairs with a fabricated
/// zero-valued spend at the same address (hence the `fvk`, which must own
/// `recipient`). The retained note is a V2 note — the Orchard pool's note
/// version is V2 under every bundle version — so the old-pool scanner picks it
/// up unchanged.
///
/// **This does not violate the turnstile.** The NU6.3 rule is
/// "`valueBalanceOrchard` MUST be nonnegative" — no *new* value may enter the old
/// pool; retaining value already inside it is explicitly fine (zebra's
/// `orchard_value_balance_non_negative`: "an Orchard bundle may still spend
/// existing notes — Orchard-to-Orchard note management nets to a zero balance").
/// Here the balance is `spends − change`, i.e. exactly the amount that crosses to
/// Ironwood, which is positive.
pub struct OldPoolChange {
    /// Full viewing key that owns `recipient`; also authorizes the fabricated
    /// paired spend through the normal signing flow.
    pub fvk: FullViewingKey,
    /// Outgoing viewing key so the sender can recover this change later.
    pub ovk: Option<OutgoingViewingKey>,
    /// Our own (internal-scope) old-pool address.
    pub recipient: Address,
    pub value_zatoshis: u64,
    pub memo: MemoBytes,
}

/// A built turnstile transaction: raw consensus bytes ready for
/// `sendrawtransaction`, plus the canonical (display-order) transaction id
/// computed by the library from the finished v6 transaction.
pub struct TurnstileTx {
    pub raw_tx: Vec<u8>,
    pub txid: String,
}

/// Errors from building a turnstile transaction.
#[derive(Debug)]
pub enum TurnstileError {
    /// The builder rejected an input/output, or `build()` failed (e.g. the
    /// value balance does not close: Ironwood outputs must total exactly
    /// `spent − fee`).
    Build(String),
    /// Serializing the finished transaction to raw bytes failed.
    Serialize(String),
}

impl std::fmt::Display for TurnstileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnstileError::Build(m) => write!(f, "turnstile build error: {m}"),
            TurnstileError::Serialize(m) => write!(f, "turnstile serialize error: {m}"),
        }
    }
}

impl std::error::Error for TurnstileError {}

/// Build a signed, proven Ironwood turnstile transaction and return its raw
/// consensus-encoded bytes (ready to hand to `sendrawtransaction`).
///
/// Value flow: each entry of `spends` is an old Orchard-pool note (V2) spent
/// through the `orchard_v3` bundle; `ironwood_outputs` receive **all** of that
/// value (payment + change) as `ironwood_v3` outputs. The caller MUST ensure
/// `sum(ironwood_outputs.value) == sum(spent note values) − fee_zatoshis`, or
/// the value balance will not close and `build()` fails.
///
/// - `params` / `target_height`: drive [`Builder::new`]'s automatic selection of
///   `orchard_v3` + `ironwood_v3` + tx v6. `target_height` MUST be ≥ the NU6.3
///   activation height reported by `params`, otherwise the builder targets the
///   pre-NU6.3 path (caller enforces this via the `PostNu63` era gate).
/// - `old_pool_anchor`: the old Orchard commitment-tree anchor the `spends`'
///   Merkle paths were built against.
/// - `spend_auth_keys`: one Orchard [`SpendAuthorizingKey`] per spend, for
///   spend-authorization signatures.
///
/// Proving is expensive (~seconds per action) as it runs the real PostNu6_3
/// circuit inside `build()`.
#[allow(clippy::too_many_arguments)]
pub fn build_turnstile_transaction(
    params: ZpayNetworkParams,
    target_height: u64,
    old_pool_anchor: Anchor,
    spends: Vec<(FullViewingKey, Note, MerklePath)>,
    spend_auth_keys: Vec<SpendAuthorizingKey>,
    ironwood_outputs: Vec<TurnstileOutput>,
    old_pool_change: Option<OldPoolChange>,
    fee_zatoshis: u64,
) -> Result<TurnstileTx, TurnstileError> {
    if spends.is_empty() {
        return Err(TurnstileError::Build(
            "turnstile requires at least one old-pool spend (nothing to cross)".to_string(),
        ));
    }
    if ironwood_outputs.is_empty() {
        return Err(TurnstileError::Build(
            "turnstile requires at least one Ironwood output (value has nowhere to land)"
                .to_string(),
        ));
    }

    let target = block_height_saturating(target_height);

    // Old pool (`orchard_v3`): spends only — value leaves via the bundle's
    // positive value balance. New pool (`ironwood_v3`): outputs only — value
    // enters via its negative value balance. `empty_tree()` is the Ironwood
    // anchor because this bundle has no Ironwood *spends* (an anchor only
    // constrains spends). `Builder::new` derives orchard_v3 + ironwood_v3 + v6
    // from `params` + `target`; we never hand-pick a bundle version.
    let build_config = BuildConfig::Standard {
        sapling_anchor: None,
        orchard_anchor: Some(old_pool_anchor),
        ironwood_anchor: Some(Anchor::empty_tree()),
        orchard_pool_bundle_type: orchard::builder::BundleType::DEFAULT,
    };

    let mut builder = Builder::new(params, target, build_config);

    for (fvk, note, merkle_path) in spends {
        builder
            .add_orchard_spend::<Infallible>(fvk, note, merkle_path)
            .map_err(|e| TurnstileError::Build(format!("add_orchard_spend failed: {e:?}")))?;
    }

    for out in ironwood_outputs {
        let value = Zatoshis::from_u64(out.value_zatoshis).map_err(|e| {
            TurnstileError::Build(format!(
                "Ironwood output value {} invalid: {e:?}",
                out.value_zatoshis
            ))
        })?;
        builder
            .add_ironwood_output::<Infallible>(out.ovk, out.recipient, value, out.memo)
            .map_err(|e| TurnstileError::Build(format!("add_ironwood_output failed: {e:?}")))?;
    }

    // Optional old-pool change: value that stays behind for a later batch. It
    // must go through add_orchard_change_output — a plain old-pool output is
    // rejected post-NU6.3 (CrossAddressDisabled) — and the builder fabricates
    // the paired zero-valued spend at the same address, authorized by the same
    // spend authorizing key as the real spends.
    if let Some(change) = old_pool_change {
        let value = Zatoshis::from_u64(change.value_zatoshis).map_err(|e| {
            TurnstileError::Build(format!(
                "old-pool change value {} invalid: {e:?}",
                change.value_zatoshis
            ))
        })?;
        builder
            .add_orchard_change_output::<Infallible>(
                change.fvk,
                change.ovk,
                change.recipient,
                value,
                change.memo,
            )
            .map_err(|e| {
                TurnstileError::Build(format!("add_orchard_change_output failed: {e:?}"))
            })?;
    }

    // Force the exact fee the proposal computed; the builder balances the two
    // pools' value balances against it. (A ZIP-317 rule would recompute the fee
    // and could disagree with the proposal's figure.)
    let fee = Zatoshis::from_u64(fee_zatoshis)
        .map_err(|e| TurnstileError::Build(format!("fee {fee_zatoshis} invalid: {e:?}")))?;
    let fee_rule = FeeRule::non_standard(fee);

    // Pure shielded turnstile: no transparent inputs, no Sapling spends. The
    // Sapling provers are structurally required but never called (fail-loud).
    let signing_set = TransparentSigningSet::new();
    let prover = FailLoudSaplingProver;

    let result = builder
        .build(
            &signing_set,
            &[], // no Sapling extended spending keys
            &spend_auth_keys,
            OsRng,
            &prover,
            &prover,
            &fee_rule,
        )
        .map_err(|e| TurnstileError::Build(format!("turnstile build failed: {e:?}")))?;

    let tx = result.transaction();
    let txid = tx.txid().to_string();
    let mut raw = Vec::new();
    tx.write(&mut raw)
        .map_err(|e| TurnstileError::Serialize(format!("transaction serialize failed: {e}")))?;
    Ok(TurnstileTx { raw_tx: raw, txid })
}

/// Build a signed, proven **Ironwood-native** transaction: Ironwood notes in,
/// Ironwood notes out, no old-pool bundle at all.
///
/// This is what spending migrated funds looks like once value lives in the new
/// pool — the ordinary shielded transfer of the post-NU6.3 world, and after a
/// completed migration the only way to spend at all.
///
/// Differences from the turnstile crossing:
/// * inputs are **V3** notes (the builder rejects any other version) anchored to
///   the **Ironwood** commitment tree, not the frozen Orchard one;
/// * there is no old-pool bundle (`orchard_anchor: None`), so the transaction
///   carries a single shielded bundle whose value balance covers the fee;
/// * the Ironwood pool *permits* cross-address transfers, so a payment to an
///   arbitrary recipient plus change back to ourselves are both plain outputs —
///   none of the old pool's same-address gymnastics apply.
///
/// The caller must ensure `sum(outputs.value) == sum(spent note values) − fee`.
/// `target_height` must be at/after NU6.3 (an Ironwood bundle exists only in v6
/// transactions); the caller's era gate enforces this.
pub fn build_ironwood_transaction(
    params: ZpayNetworkParams,
    target_height: u64,
    ironwood_anchor: Anchor,
    spends: Vec<(FullViewingKey, Note, MerklePath)>,
    spend_auth_keys: Vec<SpendAuthorizingKey>,
    outputs: Vec<TurnstileOutput>,
    fee_zatoshis: u64,
) -> Result<TurnstileTx, TurnstileError> {
    if spends.is_empty() {
        return Err(TurnstileError::Build(
            "an Ironwood spend requires at least one Ironwood note".to_string(),
        ));
    }
    if outputs.is_empty() {
        return Err(TurnstileError::Build(
            "an Ironwood spend requires at least one output (value has nowhere to land)"
                .to_string(),
        ));
    }

    let target = block_height_saturating(target_height);

    // Ironwood only: no Sapling anchor, and `orchard_anchor: None` means no
    // old-pool builder is created at all, so no old-pool bundle can appear.
    let build_config = BuildConfig::Standard {
        sapling_anchor: None,
        orchard_anchor: None,
        ironwood_anchor: Some(ironwood_anchor),
        orchard_pool_bundle_type: orchard::builder::BundleType::DEFAULT,
    };

    let mut builder = Builder::new(params, target, build_config);

    for (fvk, note, merkle_path) in spends {
        builder
            .add_ironwood_spend::<Infallible>(fvk, note, merkle_path)
            .map_err(|e| TurnstileError::Build(format!("add_ironwood_spend failed: {e:?}")))?;
    }

    for out in outputs {
        let value = Zatoshis::from_u64(out.value_zatoshis).map_err(|e| {
            TurnstileError::Build(format!(
                "Ironwood output value {} invalid: {e:?}",
                out.value_zatoshis
            ))
        })?;
        builder
            .add_ironwood_output::<Infallible>(out.ovk, out.recipient, value, out.memo)
            .map_err(|e| TurnstileError::Build(format!("add_ironwood_output failed: {e:?}")))?;
    }

    let fee = Zatoshis::from_u64(fee_zatoshis)
        .map_err(|e| TurnstileError::Build(format!("fee {fee_zatoshis} invalid: {e:?}")))?;
    let fee_rule = FeeRule::non_standard(fee);

    let signing_set = TransparentSigningSet::new();
    let prover = FailLoudSaplingProver;

    let result = builder
        .build(
            &signing_set,
            &[], // no Sapling extended spending keys
            &spend_auth_keys,
            OsRng,
            &prover,
            &prover,
            &fee_rule,
        )
        .map_err(|e| TurnstileError::Build(format!("ironwood build failed: {e:?}")))?;

    let tx = result.transaction();
    let txid = tx.txid().to_string();
    let mut raw = Vec::new();
    tx.write(&mut raw)
        .map_err(|e| TurnstileError::Serialize(format!("transaction serialize failed: {e}")))?;
    Ok(TurnstileTx { raw_tx: raw, txid })
}

/// A payout to a transparent address (the destination of a deshield).
pub struct TransparentPayout {
    pub address: TransparentAddress,
    pub value_zatoshis: u64,
}

/// Where a deshield's shielded change goes.
///
/// A deshield spends whole shielded notes, so unless a note matches the payout
/// exactly there is change, and it has to land in *some* shielded pool.
pub enum ShieldedChange {
    /// Into Ironwood — the default, and the only choice when spending Ironwood.
    /// From the old pool this also advances the migration (value leaves the
    /// closing pool).
    Ironwood(TurnstileOutput),
    /// Back into the old Orchard pool at our own address. One action cheaper
    /// than crossing (no second bundle), but the value still has to be migrated
    /// later. Only valid when the inputs are old-pool notes.
    OldPool(OldPoolChange),
}

/// Decode a transparent (t-) address into the library's [`TransparentAddress`].
///
/// Accepts both P2PKH and P2SH forms on mainnet (`t1`/`t3`) and on
/// testnet/regtest (`tm`/`t2`), verifying the Base58Check checksum. The prefix
/// decides the script type — a P2SH payout must not be encoded as P2PKH, or the
/// funds would go to an unspendable script.
pub fn decode_transparent_address(address: &str) -> Result<TransparentAddress, TurnstileError> {
    // Zcash t-address: 2-byte prefix + 20-byte hash160 + 4-byte checksum.
    const P2PKH_MAIN: [u8; 2] = [0x1C, 0xB8]; // t1
    const P2SH_MAIN: [u8; 2] = [0x1C, 0xBD]; // t3
    const P2PKH_TEST: [u8; 2] = [0x1D, 0x25]; // tm
    const P2SH_TEST: [u8; 2] = [0x1C, 0xBA]; // t2

    let decoded = bs58::decode(address)
        .into_vec()
        .map_err(|e| TurnstileError::Build(format!("invalid t-address encoding: {e}")))?;
    if decoded.len() != 26 {
        return Err(TurnstileError::Build(format!(
            "invalid t-address length: expected 26 bytes, got {}",
            decoded.len()
        )));
    }

    let (payload, checksum) = decoded.split_at(22);
    let digest = Sha256::digest(Sha256::digest(payload));
    if &digest[..4] != checksum {
        return Err(TurnstileError::Build(
            "t-address checksum mismatch".to_string(),
        ));
    }

    let mut hash = [0u8; 20];
    hash.copy_from_slice(&payload[2..22]);
    match [payload[0], payload[1]] {
        P2PKH_MAIN | P2PKH_TEST => Ok(TransparentAddress::PublicKeyHash(hash)),
        P2SH_MAIN | P2SH_TEST => Ok(TransparentAddress::ScriptHash(hash)),
        prefix => Err(TurnstileError::Build(format!(
            "unrecognised t-address prefix {:02x}{:02x}",
            prefix[0], prefix[1]
        ))),
    }
}

/// Build a signed, proven **deshield**: shielded notes in, a transparent payout
/// out, with any change staying shielded.
///
/// Needed from NU6.3 for both pools:
/// * **Ironwood → transparent** is how a wallet whose funds have migrated pays a
///   supplier at a t-address. Nothing else can do it — after a migration every
///   note is an Ironwood note.
/// * **old pool → transparent** stays consensus-legal after NU6.3 (unshielding
///   leaves `valueBalanceOrchard` positive, which the turnstile permits), so
///   there is no reason to force those notes through Ironwood first.
///
/// `source_pool` selects which bundle spends: the Orchard bundle anchored to the
/// frozen Orchard tree, or the Ironwood bundle anchored to the growing Ironwood
/// tree. `anchor` must be that pool's anchor. Change may only stay in the old
/// pool when the inputs come from it.
///
/// The caller must ensure `payout + change == sum(spent notes) − fee`.
#[allow(clippy::too_many_arguments)]
pub fn build_deshield_transaction(
    params: ZpayNetworkParams,
    target_height: u64,
    source_pool: ShieldedPool,
    anchor: Anchor,
    spends: Vec<(FullViewingKey, Note, MerklePath)>,
    spend_auth_keys: Vec<SpendAuthorizingKey>,
    payout: TransparentPayout,
    change: Option<ShieldedChange>,
    fee_zatoshis: u64,
) -> Result<TurnstileTx, TurnstileError> {
    if spends.is_empty() {
        return Err(TurnstileError::Build(
            "a deshield requires at least one shielded note to spend".to_string(),
        ));
    }
    let spends_ironwood = source_pool == ShieldedPool::Ironwood;
    if spends_ironwood && matches!(change, Some(ShieldedChange::OldPool(_))) {
        return Err(TurnstileError::Build(
            "cannot keep change in the old Orchard pool when spending Ironwood notes: the old \
             pool is closed to new value (valueBalanceOrchard must stay nonnegative)"
                .to_string(),
        ));
    }

    let target = block_height_saturating(target_height);

    // Only the pools this transaction actually uses get a builder: the spending
    // pool, plus Ironwood when the change crosses into it.
    let needs_ironwood_bundle =
        spends_ironwood || matches!(change, Some(ShieldedChange::Ironwood(_)));
    let build_config = BuildConfig::Standard {
        sapling_anchor: None,
        orchard_anchor: if spends_ironwood { None } else { Some(anchor) },
        ironwood_anchor: if !needs_ironwood_bundle {
            None
        } else if spends_ironwood {
            Some(anchor)
        } else {
            // Outputs only; an anchor constrains spends, and there are none here.
            Some(Anchor::empty_tree())
        },
        orchard_pool_bundle_type: orchard::builder::BundleType::DEFAULT,
    };

    let mut builder = Builder::new(params, target, build_config);

    for (fvk, note, merkle_path) in spends {
        if spends_ironwood {
            builder
                .add_ironwood_spend::<Infallible>(fvk, note, merkle_path)
                .map_err(|e| TurnstileError::Build(format!("add_ironwood_spend failed: {e:?}")))?;
        } else {
            builder
                .add_orchard_spend::<Infallible>(fvk, note, merkle_path)
                .map_err(|e| TurnstileError::Build(format!("add_orchard_spend failed: {e:?}")))?;
        }
    }

    let payout_value = Zatoshis::from_u64(payout.value_zatoshis).map_err(|e| {
        TurnstileError::Build(format!(
            "transparent payout value {} invalid: {e:?}",
            payout.value_zatoshis
        ))
    })?;
    builder
        .add_transparent_output(&payout.address, payout_value)
        .map_err(|e| TurnstileError::Build(format!("add_transparent_output failed: {e:?}")))?;

    match change {
        Some(ShieldedChange::Ironwood(out)) => {
            let value = Zatoshis::from_u64(out.value_zatoshis).map_err(|e| {
                TurnstileError::Build(format!(
                    "Ironwood change value {} invalid: {e:?}",
                    out.value_zatoshis
                ))
            })?;
            builder
                .add_ironwood_output::<Infallible>(out.ovk, out.recipient, value, out.memo)
                .map_err(|e| {
                    TurnstileError::Build(format!("add_ironwood_output failed: {e:?}"))
                })?;
        }
        Some(ShieldedChange::OldPool(change)) => {
            let value = Zatoshis::from_u64(change.value_zatoshis).map_err(|e| {
                TurnstileError::Build(format!(
                    "old-pool change value {} invalid: {e:?}",
                    change.value_zatoshis
                ))
            })?;
            builder
                .add_orchard_change_output::<Infallible>(
                    change.fvk,
                    change.ovk,
                    change.recipient,
                    value,
                    change.memo,
                )
                .map_err(|e| {
                    TurnstileError::Build(format!("add_orchard_change_output failed: {e:?}"))
                })?;
        }
        None => {}
    }

    let fee = Zatoshis::from_u64(fee_zatoshis)
        .map_err(|e| TurnstileError::Build(format!("fee {fee_zatoshis} invalid: {e:?}")))?;
    let fee_rule = FeeRule::non_standard(fee);

    // No transparent *inputs* (this spends shielded notes), so nothing to sign
    // on the transparent side; the Sapling provers stay fail-loud placeholders.
    let signing_set = TransparentSigningSet::new();
    let prover = FailLoudSaplingProver;

    let result = builder
        .build(
            &signing_set,
            &[],
            &spend_auth_keys,
            OsRng,
            &prover,
            &prover,
            &fee_rule,
        )
        .map_err(|e| TurnstileError::Build(format!("deshield build failed: {e:?}")))?;

    let tx = result.transaction();
    let txid = tx.txid().to_string();
    let mut raw = Vec::new();
    tx.write(&mut raw)
        .map_err(|e| TurnstileError::Serialize(format!("transaction serialize failed: {e}")))?;
    Ok(TurnstileTx { raw_tx: raw, txid })
}

/// The exact ZIP-317 fee (zatoshis) for a deshield.
///
/// Unlike the fully shielded paths this one has a transparent component, and
/// ZIP-317 charges it: `logical_actions` adds
/// `max(ceil(t_in_size/150), ceil(t_out_size/34))` on top of the shielded action
/// counts. A deshield has no transparent inputs and pays standard-size (34-byte)
/// P2PKH/P2SH outputs, so that term is simply the number of payouts.
///
/// Mirrors `zcash_primitives`' `zip317::FeeRule::fee_required` with the Sapling
/// terms zero.
pub fn deshield_fee_zatoshis(
    source_pool: ShieldedPool,
    n_spends: usize,
    change_pool: Option<ShieldedPool>,
    n_transparent_outputs: usize,
) -> Result<u64, TurnstileError> {
    use orchard::builder::BundleType;
    use orchard::bundle::BundleVersion;
    use zcash_primitives::transaction::fees::zip317::{GRACE_ACTIONS, MARGINAL_FEE};

    let spends_ironwood = source_pool == ShieldedPool::Ironwood;
    let ironwood_change = usize::from(change_pool == Some(ShieldedPool::Ironwood));
    let old_pool_change = usize::from(change_pool == Some(ShieldedPool::Orchard));

    let orchard_actions = if spends_ironwood {
        0
    } else {
        BundleType::DEFAULT
            .num_actions(
                BundleVersion::orchard_v3().default_flags(),
                n_spends,
                old_pool_change,
            )
            .map_err(|e| TurnstileError::Build(format!("orchard action count: {e}")))?
    };

    let ironwood_actions = if spends_ironwood {
        BundleType::DEFAULT
            .num_actions(
                BundleVersion::ironwood_v3().default_flags(),
                n_spends,
                ironwood_change,
            )
            .map_err(|e| TurnstileError::Build(format!("ironwood action count: {e}")))?
    } else if ironwood_change > 0 {
        // Change-only Ironwood bundle, still padded to the 2-action minimum.
        BundleType::DEFAULT
            .num_actions(
                BundleVersion::ironwood_v3().default_flags(),
                0,
                ironwood_change,
            )
            .map_err(|e| TurnstileError::Build(format!("ironwood action count: {e}")))?
    } else {
        0
    };

    let logical_actions = n_transparent_outputs + orchard_actions + ironwood_actions;
    let fee = (MARGINAL_FEE * core::cmp::max(GRACE_ACTIONS, logical_actions))
        .ok_or_else(|| TurnstileError::Build("ZIP-317 fee overflow".to_string()))?;
    Ok(u64::from(fee))
}

/// The exact ZIP-317 fee (zatoshis) for an Ironwood-native transaction with
/// `n_spends` Ironwood inputs and `n_outputs` Ironwood outputs.
///
/// Single bundle, and Ironwood *permits* cross-address transfers, so its action
/// count is `max(n_spends, n_outputs)` padded to the 2-action minimum — not the
/// `spends + outputs` sum the closed old pool is charged. A 1-in/2-out transfer
/// (payment + change) is therefore 2 actions = 10000 zatoshis, half the
/// double-bundle turnstile fee.
pub fn ironwood_fee_zatoshis(n_spends: usize, n_outputs: usize) -> Result<u64, TurnstileError> {
    use orchard::builder::BundleType;
    use orchard::bundle::BundleVersion;
    use zcash_primitives::transaction::fees::zip317::{GRACE_ACTIONS, MARGINAL_FEE};

    let actions = BundleType::DEFAULT
        .num_actions(
            BundleVersion::ironwood_v3().default_flags(),
            n_spends,
            n_outputs,
        )
        .map_err(|e| TurnstileError::Build(format!("ironwood action count: {e}")))?;

    let fee = (MARGINAL_FEE * core::cmp::max(GRACE_ACTIONS, actions))
        .ok_or_else(|| TurnstileError::Build("ZIP-317 fee overflow".to_string()))?;
    Ok(u64::from(fee))
}

/// The exact ZIP-317 fee (zatoshis) for a turnstile transaction with `n_spends`
/// old-pool spends and `n_ironwood_outputs` Ironwood outputs, no transparent or
/// Sapling components.
///
/// This mirrors `zcash_primitives`' own `get_fee`/`fee_required` exactly: each
/// bundle's action count comes from [`orchard::builder::BundleType::num_actions`]
/// — so the padding matches the wire, including the two rules that trip up a
/// hand-count:
///   * the old pool (`orchard_v3`) has cross-address **disabled**, so its action
///     count is `num_spends + num_outputs` (never `max`), where `num_outputs`
///     counts the wallet-controlled change outputs (a plain output is invalid
///     there). With no old-pool change that is `pad(n_spends → min 2)`; a
///     retained change adds one more action, because the builder pairs it with a
///     fabricated zero-valued spend;
///   * the Ironwood pool (`ironwood_v3`) pads to a 2-action minimum, so with 1–2
///     outputs it is always 2 actions (fee is therefore independent of whether an
///     Ironwood change output is present).
/// Then `fee = MARGINAL_FEE × max(GRACE_ACTIONS, orchard + ironwood)`.
///
/// The proposal's fee (a v5 single-bundle estimate) under-counts the
/// double-bundle turnstile and zebra's mempool rejects the unpaid actions
/// (`-25: Unpaid actions is higher than the limit`), so the executor recomputes
/// with this after note selection and feeds it to the fixed fee rule.
pub fn turnstile_fee_zatoshis(
    n_spends: usize,
    n_old_pool_changes: usize,
    n_ironwood_outputs: usize,
) -> Result<u64, TurnstileError> {
    use orchard::builder::BundleType;
    use orchard::bundle::BundleVersion;
    use zcash_primitives::transaction::fees::zip317::{GRACE_ACTIONS, MARGINAL_FEE};

    let orchard_actions = BundleType::DEFAULT
        .num_actions(
            BundleVersion::orchard_v3().default_flags(),
            n_spends,
            // Old-pool outputs are wallet-controlled change only (a retained
            // batch remainder); 0 for a full crossing.
            n_old_pool_changes,
        )
        .map_err(|e| TurnstileError::Build(format!("orchard action count: {e}")))?;
    let ironwood_actions = BundleType::DEFAULT
        .num_actions(
            BundleVersion::ironwood_v3().default_flags(),
            0, // no Ironwood spend
            n_ironwood_outputs,
        )
        .map_err(|e| TurnstileError::Build(format!("ironwood action count: {e}")))?;

    // No transparent / Sapling components, so logical actions are just the two
    // shielded bundles' actions summed (matches fee_required with the transparent
    // and sapling terms zero).
    let logical_actions = orchard_actions + ironwood_actions;
    let fee = (MARGINAL_FEE * core::cmp::max(GRACE_ACTIONS, logical_actions))
        .ok_or_else(|| TurnstileError::Build("ZIP-317 fee overflow".to_string()))?;
    Ok(u64::from(fee))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_report_node_activation_heights() {
        // 档B-shaped: everything at 1 except NU6.3 at 500.
        let params = ZpayNetworkParams::new(
            NetworkType::Test,
            vec![
                (NetworkUpgrade::Nu5, 1),
                (NetworkUpgrade::Nu6, 1),
                (NetworkUpgrade::Nu6_1, 1),
                (NetworkUpgrade::Nu6_2, 1),
                (NetworkUpgrade::Nu6_3, 500),
            ],
        );
        assert_eq!(params.network_type(), NetworkType::Test);
        assert_eq!(
            params.activation_height(NetworkUpgrade::Nu5),
            Some(BlockHeight::from_u32(1))
        );
        assert_eq!(
            params.activation_height(NetworkUpgrade::Nu6_3),
            Some(BlockHeight::from_u32(500))
        );
        // NU6.3 active at 500 and above, dormant below.
        assert!(params.is_nu_active(NetworkUpgrade::Nu6_3, BlockHeight::from_u32(500)));
        assert!(!params.is_nu_active(NetworkUpgrade::Nu6_3, BlockHeight::from_u32(499)));
    }

    #[test]
    fn upgrade_name_mapping_covers_nu63_aliases() {
        for name in ["NU6.3", "nu6_3", "nu63", "Ironwood"] {
            assert_eq!(
                ZpayNetworkParams::upgrade_from_name(name),
                Some(NetworkUpgrade::Nu6_3),
                "failed to map {name}"
            );
        }
        assert_eq!(ZpayNetworkParams::upgrade_from_name("bogus"), None);
    }

    #[test]
    fn block_height_saturates_instead_of_wrapping() {
        // A malformed huge height must never wrap to a small (possibly active)
        // height — it saturates to u32::MAX (an unreachable, inactive height).
        let h = block_height_saturating(u64::from(u32::MAX) + 1_000);
        assert_eq!(h, BlockHeight::from_u32(u32::MAX));
    }

    // Fail-loud contract: the Sapling prover must panic if ever invoked, rather
    // than silently emitting proof bytes. GrothProofBytes is [u8; 192] (the
    // fixed Groth16/BLS12-381 proof size: 48 + 96 + 48).
    #[test]
    #[should_panic(expected = "FailLoudSaplingProver invoked")]
    fn sapling_spend_prover_is_fail_loud() {
        let _ = <FailLoudSaplingProver as SpendProver>::encode_proof([0u8; 192]);
    }

    #[test]
    #[should_panic(expected = "FailLoudSaplingProver invoked")]
    fn sapling_output_prover_is_fail_loud() {
        let _ = <FailLoudSaplingProver as OutputProver>::encode_proof([0u8; 192]);
    }

    #[test]
    fn turnstile_fee_matches_zip317_double_bundle() {
        // The value that succeeded on 档B′ (tx 7698570627… @ height 511): 1
        // old-pool spend (padded to 2 actions) + 2 Ironwood outputs (payment +
        // change, 2 actions) = 4 logical actions × 5000 = 20000.
        assert_eq!(turnstile_fee_zatoshis(1, 0, 2).unwrap(), 20_000);
        // No change (1 Ironwood output) still pads to 2 ironwood actions → 20000.
        assert_eq!(turnstile_fee_zatoshis(1, 0, 1).unwrap(), 20_000);
        assert_eq!(turnstile_fee_zatoshis(2, 0, 2).unwrap(), 20_000);
        // 3 spends → orchard pads to 3, + 2 ironwood = 5 → 25000.
        assert_eq!(turnstile_fee_zatoshis(3, 0, 2).unwrap(), 25_000);
        // 4 spends → 4 + 2 = 6 → 30000.
        assert_eq!(turnstile_fee_zatoshis(4, 0, 2).unwrap(), 30_000);
    }

    #[test]
    fn deshield_fee_charges_the_transparent_output() {
        // ZIP-317 adds max(ceil(t_in/150), ceil(t_out/34)) to the shielded action
        // count; a deshield has no transparent inputs and one standard-size payout,
        // so that term is 1 — the fully shielded helpers would under-count by an
        // action and the mempool would reject the transaction (-25).
        //
        // Ironwood source, 1 in + 1 change = 2 ironwood actions, + 1 transparent
        // output = 3 → 15000.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Ironwood, 1, Some(ShieldedPool::Ironwood), 1)
                .unwrap(),
            15_000
        );
        // Without change the ironwood bundle still pads to 2 → 3 actions.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Ironwood, 1, None, 1).unwrap(),
            15_000
        );
        // Same shape fully shielded is one action cheaper — the difference is
        // exactly the transparent payout.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Ironwood, 1, Some(ShieldedPool::Ironwood), 1)
                .unwrap()
                - ironwood_fee_zatoshis(1, 2).unwrap(),
            5_000
        );

        // Old-pool source keeping change on the old side: 1 spend + 1 change = 2
        // orchard actions (cross-address disabled sums them), no ironwood bundle,
        // + 1 transparent = 3 → 15000.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Orchard, 1, Some(ShieldedPool::Orchard), 1)
                .unwrap(),
            15_000
        );
        // Old-pool source crossing its change into Ironwood needs a second bundle,
        // which pads to 2 actions on its own: 2 orchard + 2 ironwood + 1 transparent
        // = 5 → 25000. That is the price of advancing the migration in the same
        // transaction.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Orchard, 1, Some(ShieldedPool::Ironwood), 1)
                .unwrap(),
            25_000
        );
        // Old-pool source with no change at all: 2 orchard (minimum) + 1 = 3.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Orchard, 1, None, 1).unwrap(),
            15_000
        );
        // More inputs cost more: 3 spends + 1 old-pool change = 4 orchard + 1 = 5.
        assert_eq!(
            deshield_fee_zatoshis(ShieldedPool::Orchard, 3, Some(ShieldedPool::Orchard), 1)
                .unwrap(),
            25_000
        );
    }

    #[test]
    fn transparent_address_decoding_keeps_the_script_type() {
        // A P2SH payout decoded as P2PKH (or vice versa) pays an unspendable
        // script, so the prefix — not the length — decides the type. Vectors are
        // built by prefix + hash160 + Base58Check so the test pins our decoder
        // against the encoding rather than against itself.
        fn encode(prefix: [u8; 2], hash: [u8; 20]) -> String {
            let mut payload = prefix.to_vec();
            payload.extend_from_slice(&hash);
            let digest = Sha256::digest(Sha256::digest(&payload));
            payload.extend_from_slice(&digest[..4]);
            bs58::encode(payload).into_string()
        }

        let hash = [0x11u8; 20];
        for (prefix, first_char) in [([0x1C, 0xB8], 't'), ([0x1D, 0x25], 't')] {
            let addr = encode(prefix, hash);
            assert!(addr.starts_with(first_char));
            assert_eq!(
                decode_transparent_address(&addr).unwrap(),
                TransparentAddress::PublicKeyHash(hash),
                "{addr} should decode as P2PKH"
            );
        }
        for prefix in [[0x1C, 0xBD], [0x1C, 0xBA]] {
            let addr = encode(prefix, hash);
            assert_eq!(
                decode_transparent_address(&addr).unwrap(),
                TransparentAddress::ScriptHash(hash),
                "{addr} should decode as P2SH"
            );
        }

        // A corrupted address must be refused, not silently paid: flipping a
        // payload byte invalidates the checksum.
        let good = encode([0x1C, 0xB8], hash);
        let mut bytes = bs58::decode(&good).into_vec().unwrap();
        bytes[5] ^= 0xff;
        let corrupted = bs58::encode(bytes).into_string();
        assert!(decode_transparent_address(&corrupted).is_err());

        // An unknown prefix (here a Sapling z-address style prefix) is refused
        // rather than guessed at.
        let unknown = encode([0x16, 0x9A], hash);
        assert!(decode_transparent_address(&unknown).is_err());
    }

    #[test]
    fn ironwood_native_fee_is_single_bundle_max_not_sum() {
        // Ironwood permits cross-address transfers, so a spend and an output can
        // share an action: the count is max(spends, outputs), padded to 2.
        // 1 in / 2 out (payment + change) = 2 actions → 10000 — half the
        // double-bundle turnstile fee for the same shape.
        assert_eq!(ironwood_fee_zatoshis(1, 2).unwrap(), 10_000);
        assert_eq!(ironwood_fee_zatoshis(1, 1).unwrap(), 10_000);
        assert_eq!(ironwood_fee_zatoshis(2, 2).unwrap(), 10_000);
        // 3 in / 2 out = 3 actions → 15000 (the sum rule would say 5 → 25000).
        assert_eq!(ironwood_fee_zatoshis(3, 2).unwrap(), 15_000);
        // 2 in / 5 out (a batch of payments) = 5 actions → 25000.
        assert_eq!(ironwood_fee_zatoshis(2, 5).unwrap(), 25_000);
        // Same shape costs strictly less than crossing the turnstile.
        assert!(ironwood_fee_zatoshis(1, 2).unwrap() < turnstile_fee_zatoshis(1, 0, 2).unwrap());
    }

    #[test]
    fn old_pool_change_costs_one_more_action() {
        // Retaining change in the old pool adds an old-pool output, and with
        // cross-address disabled that output gets its own action (paired with a
        // fabricated zero-valued spend) — it is NOT folded into a spend's action.
        // 1 spend + 1 change = 2 orchard actions (already the minimum) + 2
        // ironwood = 4 → 20000, same as no change.
        assert_eq!(turnstile_fee_zatoshis(1, 1, 1).unwrap(), 20_000);
        // 2 spends + 1 change = 3 orchard actions + 2 ironwood = 5 → 25000,
        // one action more than the same spend count without retained change.
        assert_eq!(turnstile_fee_zatoshis(2, 1, 1).unwrap(), 25_000);
        assert_eq!(turnstile_fee_zatoshis(2, 0, 1).unwrap(), 20_000);
        // 3 spends + 1 change = 4 + 2 = 6 → 30000.
        assert_eq!(turnstile_fee_zatoshis(3, 1, 1).unwrap(), 30_000);
    }
}
