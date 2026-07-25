//! Unified Address generation and management
//!
//! Unified addresses (u1...) can contain receivers for multiple pools:
//! - Orchard (newest, Halo 2)
//! - Sapling (Groth16)
//! - Transparent (P2PKH)

#![allow(dead_code)]

use super::{keys::OrchardViewingKey, OrchardError, OrchardResult};
use orchard::Address as OrchardAddress;
use serde::{Deserialize, Serialize};
use zcash_address::unified::{self, Container, Encoding, Receiver};
use zcash_protocol::consensus::NetworkType;

/// Information about a unified address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAddressInfo {
    /// The unified address string (u1...)
    pub address: String,

    /// Whether it contains an Orchard receiver
    pub has_orchard: bool,

    /// Whether it contains a Sapling receiver
    pub has_sapling: bool,

    /// Whether it contains a transparent receiver
    pub has_transparent: bool,

    /// The transparent address component (if present)
    pub transparent_address: Option<String>,

    /// Address index in the HD derivation path
    pub address_index: u32,

    /// The account this address belongs to
    pub account_index: u32,
}

/// Receiver type in a unified address
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiverType {
    Orchard,
    Sapling,
    Transparent,
}

/// Transparent (P2PKH) address version prefix bytes for a given network.
/// mainnet t1 = 0x1CB8; testnet/regtest tm = 0x1D25.
fn t_addr_prefix(network: NetworkType) -> [u8; 2] {
    match network {
        NetworkType::Main => [0x1C, 0xB8],
        NetworkType::Test | NetworkType::Regtest => [0x1D, 0x25],
    }
}

/// Manager for Orchard/Unified address operations
pub struct OrchardAddressManager {
    /// The viewing key used for address derivation
    viewing_key: OrchardViewingKey,
    /// Next address index to use
    next_index: u32,
    /// Consensus network the generated addresses are encoded for
    network: NetworkType,
}

impl OrchardAddressManager {
    /// Create a new address manager from a viewing key (mainnet encoding).
    pub fn new(viewing_key: OrchardViewingKey) -> Self {
        Self::with_network(viewing_key, NetworkType::Main)
    }

    /// Create a new address manager encoding addresses for a specific network.
    pub fn with_network(viewing_key: OrchardViewingKey, network: NetworkType) -> Self {
        Self {
            viewing_key,
            next_index: 0,
            network,
        }
    }

    /// Generate a new unified address with all receivers
    ///
    /// The address will contain Orchard, Sapling, and transparent receivers
    /// for maximum compatibility.
    pub fn generate_unified_address(&mut self) -> OrchardResult<UnifiedAddressInfo> {
        let index = self.next_index;
        self.next_index += 1;

        self.generate_address_at_index(index)
    }

    /// Generate a unified address at a specific index
    pub fn generate_address_at_index(&self, index: u32) -> OrchardResult<UnifiedAddressInfo> {
        // Get the proper Orchard address from the viewing key
        let orchard_address = self.viewing_key.address_at(index);
        let orchard_receiver = orchard_address.to_raw_address_bytes();

        // For Sapling and transparent, we still use placeholders
        // In a full implementation, these would be derived from separate keys
        let sapling_receiver = self.derive_sapling_receiver_placeholder(index)?;
        let transparent_address = self.derive_transparent_address(index)?;

        // Encode as unified address
        let unified_address =
            self.encode_unified_address(&orchard_receiver, &sapling_receiver, &transparent_address)?;

        Ok(UnifiedAddressInfo {
            address: unified_address,
            has_orchard: true,
            has_sapling: true,
            has_transparent: true,
            transparent_address: Some(transparent_address),
            address_index: index,
            account_index: self.viewing_key.account_index,
        })
    }

    /// Generate an Orchard-only unified address (maximum privacy)
    pub fn generate_orchard_only_address(&self, index: u32) -> OrchardResult<UnifiedAddressInfo> {
        // Get the proper Orchard address from the viewing key
        let orchard_address = self.viewing_key.address_at(index);
        let orchard_receiver = orchard_address.to_raw_address_bytes();

        // Encode with only Orchard receiver
        let unified_address = self.encode_unified_address_single(&orchard_receiver, ReceiverType::Orchard)?;

        Ok(UnifiedAddressInfo {
            address: unified_address,
            has_orchard: true,
            has_sapling: false,
            has_transparent: false,
            transparent_address: None,
            address_index: index,
            account_index: self.viewing_key.account_index,
        })
    }

    /// Parse a unified address to extract its components
    pub fn parse_unified_address(address: &str) -> OrchardResult<UnifiedAddressInfo> {
        // u1 (mainnet), utest1 (testnet), uregtest1 (regtest) — gating on u1 alone
        // rejected every unified address on a test chain.
        if !super::transfer::UNIFIED_ADDRESS_PREFIXES
            .iter()
            .any(|prefix| address.starts_with(prefix))
        {
            return Err(OrchardError::InvalidUnifiedAddress(format!(
                "Unified address must start with one of {:?}",
                super::transfer::UNIFIED_ADDRESS_PREFIXES
            )));
        }

        // Decode and parse the unified address
        // In a real implementation, this would use F4Jumble decoding
        let (network, decoded) = Self::decode_unified_address(address)?;

        let has_orchard = decoded.iter().any(|(t, _)| *t == ReceiverType::Orchard);
        let has_sapling = decoded.iter().any(|(t, _)| *t == ReceiverType::Sapling);
        let has_transparent = decoded.iter().any(|(t, _)| *t == ReceiverType::Transparent);

        let transparent_address = decoded
            .iter()
            .find(|(t, _)| *t == ReceiverType::Transparent)
            .map(|(_, data)| Self::encode_transparent_address(data, network));

        Ok(UnifiedAddressInfo {
            address: address.to_string(),
            has_orchard,
            has_sapling,
            has_transparent,
            transparent_address,
            address_index: 0, // Unknown when parsing external address
            account_index: 0,
        })
    }

    /// Validate a unified address
    pub fn validate_unified_address(address: &str) -> bool {
        if !super::transfer::is_unified_address(address) {
            return false;
        }

        // Try to decode - if successful, it's valid
        Self::decode_unified_address(address).is_ok()
    }

    /// Extract Orchard address from a unified address
    /// Returns the orchard::Address if the unified address contains an Orchard receiver
    pub fn extract_orchard_address(address: &str) -> OrchardResult<OrchardAddress> {
        let (_network, receivers) = Self::decode_unified_address(address)?;

        // Find Orchard receiver (typecode 0x03, 43 bytes: 11 diversifier + 32 pk_d)
        let orchard_data = receivers
            .iter()
            .find(|(t, _)| *t == ReceiverType::Orchard)
            .map(|(_, data)| data)
            .ok_or_else(|| {
                OrchardError::InvalidUnifiedAddress(
                    "Unified address does not contain an Orchard receiver".to_string(),
                )
            })?;

        if orchard_data.len() != 43 {
            return Err(OrchardError::InvalidUnifiedAddress(format!(
                "Invalid Orchard receiver length: expected 43, got {}",
                orchard_data.len()
            )));
        }

        // Parse diversifier (11 bytes)
        let mut diversifier_bytes = [0u8; 11];
        diversifier_bytes.copy_from_slice(&orchard_data[..11]);
        let _diversifier = orchard::keys::Diversifier::from_bytes(diversifier_bytes);

        // Parse pk_d (32 bytes)
        let mut pk_d_bytes = [0u8; 32];
        pk_d_bytes.copy_from_slice(&orchard_data[11..43]);

        // Convert to orchard::Address
        OrchardAddress::from_raw_address_bytes(&orchard_data[..].try_into().unwrap())
            .into_option()
            .ok_or_else(|| {
                OrchardError::InvalidUnifiedAddress("Invalid Orchard address encoding".to_string())
            })
    }

    /// Derive Sapling receiver placeholder from index
    /// Note: This is a placeholder - real Sapling support would require separate Sapling keys
    fn derive_sapling_receiver_placeholder(&self, index: u32) -> OrchardResult<Vec<u8>> {
        // Sapling receiver is 43 bytes
        // This is a placeholder - real implementation would use Sapling keys
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(43)
            .personal(b"ZcashSaplingRcv")
            .to_state();
        hasher.update(&self.viewing_key.fvk_bytes());
        hasher.update(&index.to_le_bytes());
        let result = hasher.finalize();

        Ok(result.as_bytes().to_vec())
    }

    /// Derive transparent address from index
    /// Note: This is a placeholder - real implementation would use transparent keys
    fn derive_transparent_address(&self, index: u32) -> OrchardResult<String> {
        use ripemd::Ripemd160;
        use sha2::{Digest, Sha256};

        // Derive public key hash for transparent address
        // This is a placeholder - real implementation would use transparent keys
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(33) // Compressed public key size
            .personal(b"ZcashOrchardTPK")
            .to_state();
        hasher.update(&self.viewing_key.fvk_bytes());
        hasher.update(&index.to_le_bytes());
        let pubkey = hasher.finalize();

        // Hash160 (SHA256 + RIPEMD160)
        let sha256_hash = Sha256::digest(pubkey.as_bytes());
        let hash160 = Ripemd160::digest(&sha256_hash);

        // Encode as t-address with the network-appropriate prefix
        let mut payload = t_addr_prefix(self.network).to_vec();
        payload.extend_from_slice(&hash160);

        // Add checksum
        let checksum = Sha256::digest(&Sha256::digest(&payload));
        payload.extend_from_slice(&checksum[..4]);

        Ok(bs58::encode(payload).into_string())
    }

    /// Encode unified address from receivers using the proper zcash_address crate
    fn encode_unified_address(
        &self,
        orchard_receiver: &[u8],
        sapling_receiver: &[u8],
        transparent_address: &str,
    ) -> OrchardResult<String> {
        let mut receivers = Vec::new();

        // Add Orchard receiver (43 bytes)
        if orchard_receiver.len() == 43 {
            let mut orchard_data = [0u8; 43];
            orchard_data.copy_from_slice(orchard_receiver);
            receivers.push(Receiver::Orchard(orchard_data));
        }

        // Add Sapling receiver (43 bytes)
        if sapling_receiver.len() == 43 {
            let mut sapling_data = [0u8; 43];
            sapling_data.copy_from_slice(sapling_receiver);
            receivers.push(Receiver::Sapling(sapling_data));
        }

        // Add transparent receiver (decode t-address to get pubkey hash)
        if let Ok(decoded) = bs58::decode(transparent_address).into_vec() {
            if decoded.len() >= 22 {
                let mut pubkey_hash = [0u8; 20];
                pubkey_hash.copy_from_slice(&decoded[2..22]);
                receivers.push(Receiver::P2pkh(pubkey_hash));
            }
        }

        // Build unified address
        let ua = unified::Address::try_from_items(receivers).map_err(|e| {
            OrchardError::InvalidUnifiedAddress(format!("Failed to create unified address: {:?}", e))
        })?;

        // Encode for the manager's network
        let address = ua.encode(&self.network);

        Ok(address)
    }

    /// Encode unified address with single receiver using the proper zcash_address crate
    fn encode_unified_address_single(
        &self,
        receiver: &[u8],
        receiver_type: ReceiverType,
    ) -> OrchardResult<String> {
        let receivers = match receiver_type {
            ReceiverType::Orchard => {
                if receiver.len() != 43 {
                    return Err(OrchardError::InvalidUnifiedAddress(
                        "Orchard receiver must be 43 bytes".to_string(),
                    ));
                }
                let mut data = [0u8; 43];
                data.copy_from_slice(receiver);
                vec![Receiver::Orchard(data)]
            }
            ReceiverType::Sapling => {
                if receiver.len() != 43 {
                    return Err(OrchardError::InvalidUnifiedAddress(
                        "Sapling receiver must be 43 bytes".to_string(),
                    ));
                }
                let mut data = [0u8; 43];
                data.copy_from_slice(receiver);
                vec![Receiver::Sapling(data)]
            }
            ReceiverType::Transparent => {
                if receiver.len() != 20 {
                    return Err(OrchardError::InvalidUnifiedAddress(
                        "Transparent receiver must be 20 bytes".to_string(),
                    ));
                }
                let mut data = [0u8; 20];
                data.copy_from_slice(receiver);
                vec![Receiver::P2pkh(data)]
            }
        };

        let ua = unified::Address::try_from_items(receivers).map_err(|e| {
            OrchardError::InvalidUnifiedAddress(format!("Failed to create unified address: {:?}", e))
        })?;

        Ok(ua.encode(&self.network))
    }

    /// Decode unified address to receivers using the proper zcash_address crate.
    /// Returns the network the address was encoded for alongside its receivers.
    fn decode_unified_address(
        address: &str,
    ) -> OrchardResult<(NetworkType, Vec<(ReceiverType, Vec<u8>)>)> {
        // Parse the unified address using zcash_address crate
        let (network, ua) = unified::Address::decode(address).map_err(|e| {
            OrchardError::InvalidUnifiedAddress(format!("Failed to decode address: {:?}", e))
        })?;

        // Extract receivers
        let mut receivers = Vec::new();

        for receiver in ua.items() {
            match receiver {
                Receiver::Orchard(data) => {
                    receivers.push((ReceiverType::Orchard, data.to_vec()));
                }
                Receiver::Sapling(data) => {
                    receivers.push((ReceiverType::Sapling, data.to_vec()));
                }
                Receiver::P2pkh(data) => {
                    receivers.push((ReceiverType::Transparent, data.to_vec()));
                }
                Receiver::P2sh(data) => {
                    receivers.push((ReceiverType::Transparent, data.to_vec()));
                }
                _ => {
                    // Unknown receiver type, skip
                }
            }
        }

        if receivers.is_empty() {
            return Err(OrchardError::InvalidUnifiedAddress(
                "No valid receivers found".to_string(),
            ));
        }

        Ok((network, receivers))
    }

    /// Encode transparent address from pubkey hash for the given network
    fn encode_transparent_address(pubkey_hash: &[u8], network: NetworkType) -> String {
        use sha2::{Digest, Sha256};

        let mut payload = t_addr_prefix(network).to_vec();
        payload.extend_from_slice(pubkey_hash);

        let checksum = Sha256::digest(&Sha256::digest(&payload));
        payload.extend_from_slice(&checksum[..4]);

        bs58::encode(payload).into_string()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::zcash::orchard::keys::OrchardKeyManager;

    #[test]
    fn test_generate_unified_address() {
        let seed = vec![0u8; 64];
        let (_, vk) = OrchardKeyManager::derive_from_seed(&seed, 0, 2000000).unwrap();

        let mut manager = OrchardAddressManager::new(vk);
        let addr_info = manager.generate_unified_address().unwrap();

        assert!(addr_info.address.starts_with("u1"));
        assert!(addr_info.has_orchard);
        assert!(addr_info.has_sapling);
        assert!(addr_info.has_transparent);
        assert!(addr_info.transparent_address.is_some());
    }

    #[test]
    fn test_generate_multiple_addresses() {
        let seed = vec![1u8; 64];
        let (_, vk) = OrchardKeyManager::derive_from_seed(&seed, 0, 2000000).unwrap();

        let mut manager = OrchardAddressManager::new(vk);

        let addr1 = manager.generate_unified_address().unwrap();
        let addr2 = manager.generate_unified_address().unwrap();

        assert_ne!(addr1.address, addr2.address);
        assert_eq!(addr1.address_index, 0);
        assert_eq!(addr2.address_index, 1);
    }

    #[test]
    fn test_orchard_only_address() {
        let seed = vec![2u8; 64];
        let (_, vk) = OrchardKeyManager::derive_from_seed(&seed, 0, 2000000).unwrap();

        let manager = OrchardAddressManager::new(vk);
        let addr_info = manager.generate_orchard_only_address(0).unwrap();

        assert!(addr_info.address.starts_with("u1"));
        assert!(addr_info.has_orchard);
        assert!(!addr_info.has_sapling);
        assert!(!addr_info.has_transparent);
    }
}
