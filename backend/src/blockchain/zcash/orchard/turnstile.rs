//! Ironwood turnstile transaction construction (NU6.3+).
//!
//! After NU6.3 activates, spending an old Orchard-pool note crosses the
//! "turnstile" into the Ironwood pool: a v6 transaction carrying an
//! `orchard_v3` spend bundle (old pool, value out) and an `ironwood_v3` output
//! bundle (new pool, value in), balanced through the per-pool value balances.
//!
//! We build this via `zcash_primitives`' `TransactionBuilder` (which handles v6
//! serialization, sighash_v6, proofs, and the cross-pool value balance) rather
//! than hand-rolling v6 — a hand-rolled sighash/serialization slip on a
//! consensus-critical path means stuck or lost funds.
//!
//! This module is engaged ONLY at [`ProtocolEra::PostNu63`]; the pre-NU6.3
//! Orchard-pool path in `transfer.rs` is left completely untouched (it is
//! live-verified and zero-regression). See the F4.0-c P1 plan.

#![allow(dead_code)] // wired in as the turnstile build path lands piece by piece

use zcash_protocol::consensus::{BlockHeight, NetworkType, NetworkUpgrade, Parameters};

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
            .map(|(_, h)| BlockHeight::from_u32(*h as u32))
    }
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
}
