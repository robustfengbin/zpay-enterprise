//! Ironwood turnstile transaction construction (NU6.3+).
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

use sapling::bundle::GrothProofBytes;
use sapling::prover::{OutputProver, SpendProver};

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
