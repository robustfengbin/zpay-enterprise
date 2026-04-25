//! Orchard key management
//!
//! This module handles UnifiedSpendingKey derivation and management
//! following ZIP 32 hierarchical deterministic key derivation.

#![allow(dead_code)]

use super::{OrchardError, OrchardResult};
use orchard::keys::{FullViewingKey, Scope, SpendingKey};
use sha2::{Digest, Sha256};
use zcash_protocol::consensus::{MainNetwork, NetworkConstants};
use zip32::AccountId;

/// Orchard viewing key for scanning blocks
#[derive(Debug, Clone)]
pub struct OrchardViewingKey {
    /// The full viewing key (actual orchard FVK)
    fvk: FullViewingKey,
    /// Account index
    pub account_index: u32,
    /// Birthday height (first block to scan from)
    pub birthday_height: u64,
    /// Wallet ID (set when registered with sync service)
    pub wallet_id: Option<i32>,
}

impl OrchardViewingKey {
    /// Create from an orchard FullViewingKey
    pub fn from_fvk(fvk: FullViewingKey, account_index: u32, birthday_height: u64) -> Self {
        Self {
            fvk,
            account_index,
            birthday_height,
            wallet_id: None,
        }
    }

    /// Set wallet ID (called when registering with sync service)
    pub fn with_wallet_id(mut self, wallet_id: i32) -> Self {
        self.wallet_id = Some(wallet_id);
        self
    }

    /// Get the full viewing key
    pub fn fvk(&self) -> &FullViewingKey {
        &self.fvk
    }

    /// Get the full viewing key bytes (for compatibility)
    pub fn fvk_bytes(&self) -> Vec<u8> {
        self.fvk.to_bytes().to_vec()
    }

    /// Derive an Orchard address at the given diversifier index
    pub fn address_at(&self, diversifier_index: u32) -> orchard::Address {
        // Create diversifier from index
        let diversifier = orchard::keys::Diversifier::from_bytes(
            Self::derive_diversifier_bytes(diversifier_index)
        );
        self.fvk.address(diversifier, Scope::External)
    }

    /// Derive diversifier bytes from index
    fn derive_diversifier_bytes(index: u32) -> [u8; 11] {
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(11)
            .personal(b"Zcash_Orchard_D")
            .to_state();
        hasher.update(&index.to_le_bytes());
        let result = hasher.finalize();

        let mut diversifier = [0u8; 11];
        diversifier.copy_from_slice(result.as_bytes());
        diversifier
    }

    /// Encode the viewing key to a string representation
    pub fn encode(&self) -> String {
        // Encode as hex with metadata prefix
        format!(
            "ufvk:{}:{}:{}",
            self.account_index,
            self.birthday_height,
            hex::encode(self.fvk.to_bytes())
        )
    }

    /// Decode a viewing key from string representation
    pub fn decode(encoded: &str) -> OrchardResult<Self> {
        let parts: Vec<&str> = encoded.split(':').collect();
        if parts.len() != 4 || parts[0] != "ufvk" {
            return Err(OrchardError::KeyDerivation(
                "Invalid viewing key format".to_string(),
            ));
        }

        let account_index = parts[1]
            .parse()
            .map_err(|_| OrchardError::KeyDerivation("Invalid account index".to_string()))?;

        let birthday_height = parts[2]
            .parse()
            .map_err(|_| OrchardError::KeyDerivation("Invalid birthday height".to_string()))?;

        let fvk_bytes = hex::decode(parts[3])
            .map_err(|_| OrchardError::KeyDerivation("Invalid FVK hex".to_string()))?;

        if fvk_bytes.len() != 96 {
            return Err(OrchardError::KeyDerivation(
                format!("Invalid FVK length: expected 96, got {}", fvk_bytes.len()),
            ));
        }

        let fvk_array: [u8; 96] = fvk_bytes.try_into().unwrap();
        let fvk = FullViewingKey::from_bytes(&fvk_array)
            .ok_or_else(|| OrchardError::KeyDerivation("Invalid FVK bytes".to_string()))?;

        Ok(Self {
            fvk,
            account_index,
            birthday_height,
            wallet_id: None,
        })
    }
}

/// Orchard spending key for signing transactions
pub struct OrchardSpendingKey {
    /// The actual orchard spending key
    sk: SpendingKey,
    /// Account index
    pub account_index: u32,
}

impl OrchardSpendingKey {
    /// Create from an orchard SpendingKey
    pub fn from_sk(sk: SpendingKey, account_index: u32) -> Self {
        Self { sk, account_index }
    }

    /// Get the spending key
    pub fn sk(&self) -> &SpendingKey {
        &self.sk
    }

    /// Get the spending key bytes (use with caution)
    pub fn sk_bytes(&self) -> [u8; 32] {
        *self.sk.to_bytes()
    }

    /// Get the OutgoingViewingKey for encrypting outgoing transaction notes
    /// This allows the sender to later decrypt their own sent transactions
    pub fn to_ovk(&self) -> orchard::keys::OutgoingViewingKey {
        let fvk = FullViewingKey::from(&self.sk);
        fvk.to_ovk(Scope::External)
    }

    /// Get the FullViewingKey
    pub fn to_fvk(&self) -> FullViewingKey {
        FullViewingKey::from(&self.sk)
    }
}

/// Orchard key manager for HD key derivation
pub struct OrchardKeyManager;

impl OrchardKeyManager {
    /// Derive Orchard keys from a seed phrase using proper ZIP 32 derivation
    ///
    /// # Arguments
    /// * `seed` - 64-byte seed from BIP39 mnemonic
    /// * `account_index` - Account number (0 for first account)
    /// * `birthday_height` - Block height when wallet was created
    ///
    /// # Returns
    /// * Tuple of (spending_key, viewing_key)
    pub fn derive_from_seed(
        seed: &[u8],
        account_index: u32,
        birthday_height: u64,
    ) -> OrchardResult<(OrchardSpendingKey, OrchardViewingKey)> {
        if seed.len() < 32 {
            return Err(OrchardError::KeyDerivation(
                "Seed must be at least 32 bytes".to_string(),
            ));
        }

        // Use the orchard crate's proper ZIP 32 derivation
        // SpendingKey::from_zip32_seed derives at path m/32'/133'/account'
        let coin_type = MainNetwork.coin_type();
        let account_id = AccountId::try_from(account_index)
            .map_err(|_| OrchardError::KeyDerivation("Invalid account index".to_string()))?;
        let sk = SpendingKey::from_zip32_seed(seed, coin_type, account_id)
            .map_err(|e| OrchardError::KeyDerivation(format!("Failed to derive spending key: {:?}", e)))?;

        // Derive FullViewingKey from SpendingKey
        let fvk = FullViewingKey::from(&sk);

        let spending_key = OrchardSpendingKey::from_sk(sk, account_index);
        let viewing_key = OrchardViewingKey::from_fvk(fvk, account_index, birthday_height);

        Ok((spending_key, viewing_key))
    }

    /// Derive from an existing transparent private key (hex)
    ///
    /// This allows users to "upgrade" their transparent wallet to support Orchard
    /// by using the private key as seed material.
    pub fn derive_from_private_key(
        private_key_hex: &str,
        account_index: u32,
        birthday_height: u64,
    ) -> OrchardResult<(OrchardSpendingKey, OrchardViewingKey)> {
        let pk_bytes = hex::decode(private_key_hex)
            .map_err(|e| OrchardError::KeyDerivation(format!("Invalid private key hex: {}", e)))?;

        if pk_bytes.len() != 32 {
            return Err(OrchardError::KeyDerivation(
                "Private key must be 32 bytes".to_string(),
            ));
        }

        // Expand the 32-byte private key to a 64-byte seed using BLAKE2b
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(64)
            .personal(b"ZcashOrchardSeed")
            .to_state();
        hasher.update(&pk_bytes);
        let seed = hasher.finalize();

        Self::derive_from_seed(seed.as_bytes(), account_index, birthday_height)
    }

    /// Derive a viewing key only (for watch-only wallets)
    pub fn derive_viewing_key(
        seed: &[u8],
        account_index: u32,
        birthday_height: u64,
    ) -> OrchardResult<OrchardViewingKey> {
        let (_, viewing_key) = Self::derive_from_seed(seed, account_index, birthday_height)?;
        Ok(viewing_key)
    }

    /// Generate a random Orchard seed
    pub fn generate_seed() -> OrchardResult<Vec<u8>> {
        use rand::RngCore;
        let mut seed = vec![0u8; 64];
        rand::thread_rng().fill_bytes(&mut seed);
        Ok(seed)
    }

    /// Convert seed to BIP39 mnemonic words
    pub fn seed_to_mnemonic(seed: &[u8]) -> OrchardResult<String> {
        // Simple implementation - in production, use a proper BIP39 library
        // This is a placeholder that returns the seed as hex
        Ok(hex::encode(seed))
    }

    /// Get the fingerprint of a viewing key (for identification)
    pub fn get_fingerprint(viewing_key: &OrchardViewingKey) -> String {
        let mut hasher = Sha256::new();
        hasher.update(viewing_key.fvk_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..8])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_from_seed() {
        let seed = vec![0u8; 64];
        let (sk, vk) = OrchardKeyManager::derive_from_seed(&seed, 0, 2000000).unwrap();

        assert_eq!(sk.account_index, 0);
        assert_eq!(vk.account_index, 0);
        assert_eq!(vk.birthday_height, 2000000);
        assert!(!vk.fvk_bytes().is_empty());
    }

    #[test]
    fn test_viewing_key_encode_decode() {
        let seed = vec![1u8; 64];
        let (_, vk) = OrchardKeyManager::derive_from_seed(&seed, 0, 2000000).unwrap();

        let encoded = vk.encode();
        let decoded = OrchardViewingKey::decode(&encoded).unwrap();

        assert_eq!(vk.account_index, decoded.account_index);
        assert_eq!(vk.birthday_height, decoded.birthday_height);
        assert_eq!(vk.fvk_bytes(), decoded.fvk_bytes());
    }

    #[test]
    fn test_derive_from_private_key() {
        let private_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let (sk, vk) = OrchardKeyManager::derive_from_private_key(private_key, 0, 2000000).unwrap();

        assert_eq!(sk.account_index, 0);
        assert!(!vk.fvk_bytes().is_empty());
    }
}
