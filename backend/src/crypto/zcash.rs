#![allow(dead_code)]

use rand::RngCore;
use ripemd::Ripemd160;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};

use zcash_protocol::consensus::NetworkType;

use crate::blockchain::zcash::orchard::{
    address::OrchardAddressManager, keys::OrchardKeyManager, OrchardViewingKey, UnifiedAddressInfo,
};
use crate::error::{AppError, AppResult};

/// Zcash mainnet transparent address prefix (t1)
const ZCASH_T_ADDR_PREFIX: [u8; 2] = [0x1C, 0xB8];

/// Map a node chain moniker ("main"/"test"/"regtest") to a consensus network.
/// Unknown/absent monikers default to mainnet.
pub fn consensus_network_from_kind(kind: &str) -> NetworkType {
    match kind {
        "test" => NetworkType::Test,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Main,
    }
}

/// Transparent (P2PKH) address version prefix for a network.
/// mainnet t1 = 0x1CB8; testnet/regtest tm = 0x1D25.
fn t_addr_prefix(network: NetworkType) -> [u8; 2] {
    match network {
        NetworkType::Main => ZCASH_T_ADDR_PREFIX,
        NetworkType::Test | NetworkType::Regtest => [0x1D, 0x25],
    }
}

/// Generate a new Zcash transparent address and private key
/// Returns (address, private_key_hex)
pub fn generate_zcash_wallet(network: NetworkType) -> AppResult<(String, String)> {
    let secp = Secp256k1::new();

    // Generate random 32-byte private key
    let mut rng = rand::thread_rng();
    let mut key_bytes = [0u8; 32];
    rng.fill_bytes(&mut key_bytes);

    let secret_key = SecretKey::from_slice(&key_bytes)
        .map_err(|e| AppError::InternalError(format!("Failed to generate secret key: {}", e)))?;

    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    // Generate address from public key
    let address = public_key_to_t_address(&public_key, network)?;
    let private_key_hex = hex::encode(key_bytes);

    Ok((address, private_key_hex))
}

/// Import a Zcash wallet from private key
/// Supports both WIF format (starts with 5, K, L) and raw hex format
/// Returns the address derived from the private key
pub fn import_zcash_wallet(private_key: &str, network: NetworkType) -> AppResult<String> {
    tracing::debug!("Importing Zcash wallet, key length: {}, first char: {:?}",
        private_key.len(),
        private_key.chars().next()
    );

    let secp = Secp256k1::new();

    // Determine key format and parse
    let key_bytes = if private_key.starts_with('5') || private_key.starts_with('K') || private_key.starts_with('L') {
        // WIF format
        tracing::debug!("Detected WIF format private key");
        decode_wif_private_key(private_key)?
    } else {
        // Hex format
        tracing::debug!("Detected hex format private key");
        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        hex::decode(key_hex)
            .map_err(|e| {
                tracing::error!("Failed to decode hex private key: {}", e);
                AppError::ValidationError(format!("Invalid private key hex: {}", e))
            })?
    };

    tracing::debug!("Parsed key bytes length: {}", key_bytes.len());

    if key_bytes.len() != 32 {
        tracing::error!("Invalid key length: {} bytes", key_bytes.len());
        return Err(AppError::ValidationError(format!(
            "Private key must be 32 bytes, got {} bytes",
            key_bytes.len()
        )));
    }

    let secret_key = SecretKey::from_slice(&key_bytes)
        .map_err(|e| {
            tracing::error!("Failed to create secret key: {}", e);
            AppError::ValidationError(format!("Invalid private key: {}", e))
        })?;

    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    // Generate address from public key
    let address = public_key_to_t_address(&public_key, network)?;
    tracing::info!("Successfully derived Zcash address: {}", address);
    Ok(address)
}

/// Decode a WIF (Wallet Import Format) private key
/// WIF format: Base58Check(prefix + privkey + [compression flag] + checksum)
fn decode_wif_private_key(wif: &str) -> AppResult<Vec<u8>> {
    let decoded = bs58::decode(wif)
        .into_vec()
        .map_err(|e| AppError::ValidationError(format!("Invalid WIF format: {}", e)))?;

    // WIF uncompressed: 1 byte prefix + 32 bytes key + 4 bytes checksum = 37 bytes
    // WIF compressed: 1 byte prefix + 32 bytes key + 1 byte flag + 4 bytes checksum = 38 bytes
    if decoded.len() != 37 && decoded.len() != 38 {
        return Err(AppError::ValidationError(format!(
            "Invalid WIF length: expected 37 or 38 bytes, got {}",
            decoded.len()
        )));
    }

    // Verify checksum
    let payload_len = decoded.len() - 4;
    let payload = &decoded[..payload_len];
    let checksum = &decoded[payload_len..];

    let hash1 = Sha256::digest(payload);
    let hash2 = Sha256::digest(&hash1);

    if &hash2[..4] != checksum {
        return Err(AppError::ValidationError("WIF checksum mismatch".to_string()));
    }

    // Extract private key (skip prefix byte)
    // For mainnet: prefix is 0x80
    // For Zcash mainnet: prefix is also 0x80
    if payload[0] != 0x80 {
        return Err(AppError::ValidationError(format!(
            "Invalid WIF prefix: expected 0x80, got 0x{:02x}",
            payload[0]
        )));
    }

    // Return the 32-byte private key
    Ok(payload[1..33].to_vec())
}

/// Convert a secp256k1 public key to a Zcash transparent address (t-address)
fn public_key_to_t_address(public_key: &PublicKey, network: NetworkType) -> AppResult<String> {
    // Get compressed public key bytes
    let pubkey_bytes = public_key.serialize();

    // SHA256 hash
    let sha256_hash = Sha256::digest(&pubkey_bytes);

    // RIPEMD160 hash
    let ripemd_hash = Ripemd160::digest(&sha256_hash);

    // Build payload: network prefix + ripemd160 hash
    let mut payload = Vec::with_capacity(22);
    payload.extend_from_slice(&t_addr_prefix(network));
    payload.extend_from_slice(&ripemd_hash);

    // Double SHA256 for checksum
    let checksum1 = Sha256::digest(&payload);
    let checksum2 = Sha256::digest(&checksum1);

    // Take first 4 bytes of checksum
    let checksum = &checksum2[..4];

    // Build final address: payload + checksum
    let mut address_bytes = payload;
    address_bytes.extend_from_slice(checksum);

    // Base58 encode
    let address = bs58::encode(address_bytes).into_string();

    Ok(address)
}

/// Validate a Zcash address format
#[allow(dead_code)]
pub(crate) fn validate_zcash_address(address: &str) -> bool {
    if address.is_empty() {
        return false;
    }

    // Transparent addresses: t1/t3 (mainnet), tm/t2 (testnet/regtest). t2 (P2SH)
    // was missing, so a testnet P2SH address failed validation outright.
    if crate::blockchain::zcash::orchard::transfer::TRANSPARENT_ADDRESS_PREFIXES
        .iter()
        .any(|prefix| address.starts_with(prefix))
    {
        // Try to decode and verify checksum
        if let Ok(decoded) = bs58::decode(address).into_vec() {
            if decoded.len() == 26 {
                // 2 byte prefix + 20 byte hash + 4 byte checksum
                let payload = &decoded[..22];
                let checksum = &decoded[22..];

                let checksum1 = Sha256::digest(payload);
                let checksum2 = Sha256::digest(&checksum1);

                return &checksum2[..4] == checksum;
            }
        }
        return false;
    }

    // Sapling shielded addresses (zs)
    if address.starts_with("zs") && address.len() >= 78 {
        return true; // Basic length check for Sapling addresses
    }

    // Sprout shielded addresses (zc) - legacy
    if address.starts_with("zc") && address.len() >= 95 {
        return true;
    }

    // Unified addresses (u1 mainnet / utest1 / uregtest1)
    if is_unified_address(address) {
        return true;
    }

    false
}

/// Enable Orchard for an existing wallet by deriving Orchard keys from transparent private key
///
/// # Arguments
/// * `private_key_hex` - The transparent wallet's private key in hex format
/// * `birthday_height` - Block height when wallet was created (for scanning)
///
/// # Returns
/// * Tuple of (unified_address, viewing_key_encoded)
pub fn enable_orchard_for_wallet(
    private_key_hex: &str,
    birthday_height: u64,
    network: NetworkType,
) -> AppResult<(UnifiedAddressInfo, String)> {
    // Derive Orchard keys from the transparent private key
    let (spending_key, viewing_key) =
        OrchardKeyManager::derive_from_private_key(private_key_hex, 0, birthday_height)
            .map_err(|e| AppError::InternalError(format!("Failed to derive Orchard keys: {}", e)))?;

    // Create address manager and generate unified address
    let mut address_manager = OrchardAddressManager::with_network(viewing_key.clone(), network);
    let unified_address = address_manager
        .generate_unified_address()
        .map_err(|e| AppError::InternalError(format!("Failed to generate unified address: {}", e)))?;

    // Encode the viewing key for storage
    let viewing_key_encoded = viewing_key.encode();

    // Zero out spending key by dropping it
    drop(spending_key);

    Ok((unified_address, viewing_key_encoded))
}

/// Generate a new Orchard-enabled wallet from seed
///
/// # Arguments
/// * `seed` - 64-byte seed (e.g., from BIP39 mnemonic)
/// * `birthday_height` - Current block height
///
/// # Returns
/// * Tuple of (unified_address, transparent_address, private_key_hex, viewing_key_encoded)
pub fn generate_orchard_wallet(
    seed: &[u8],
    birthday_height: u64,
    network: NetworkType,
) -> AppResult<(UnifiedAddressInfo, String, String, String)> {
    if seed.len() < 32 {
        return Err(AppError::ValidationError(
            "Seed must be at least 32 bytes".to_string(),
        ));
    }

    // Generate transparent key from seed
    let secp = Secp256k1::new();

    let mut hasher = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(b"ZcashTransparent")
        .to_state();
    hasher.update(seed);
    let key_bytes = hasher.finalize();

    let secret_key = SecretKey::from_slice(key_bytes.as_bytes())
        .map_err(|e| AppError::InternalError(format!("Failed to generate secret key: {}", e)))?;

    let public_key = PublicKey::from_secret_key(&secp, &secret_key);
    let transparent_address = public_key_to_t_address(&public_key, network)?;
    let private_key_hex = hex::encode(key_bytes.as_bytes());

    // Derive Orchard keys
    let (_, viewing_key) = OrchardKeyManager::derive_from_seed(seed, 0, birthday_height)
        .map_err(|e| AppError::InternalError(format!("Failed to derive Orchard keys: {}", e)))?;

    // Generate unified address
    let mut address_manager = OrchardAddressManager::with_network(viewing_key.clone(), network);
    let unified_address = address_manager
        .generate_unified_address()
        .map_err(|e| AppError::InternalError(format!("Failed to generate unified address: {}", e)))?;

    let viewing_key_encoded = viewing_key.encode();

    Ok((
        unified_address,
        transparent_address,
        private_key_hex,
        viewing_key_encoded,
    ))
}

/// Generate a new unified address for an existing Orchard account
///
/// # Arguments
/// * `viewing_key_encoded` - The encoded viewing key
/// * `address_index` - The address index to generate
///
/// # Returns
/// * UnifiedAddressInfo for the new address
pub fn generate_unified_address(
    viewing_key_encoded: &str,
    address_index: u32,
    network: NetworkType,
) -> AppResult<UnifiedAddressInfo> {
    let viewing_key = OrchardViewingKey::decode(viewing_key_encoded)
        .map_err(|e| AppError::ValidationError(format!("Invalid viewing key: {}", e)))?;

    let address_manager = OrchardAddressManager::with_network(viewing_key, network);
    let address_info = address_manager
        .generate_address_at_index(address_index)
        .map_err(|e| AppError::InternalError(format!("Failed to generate address: {}", e)))?;

    Ok(address_info)
}

/// Parse and validate a unified address
///
/// # Arguments
/// * `address` - The unified address string (u1...)
///
/// # Returns
/// * UnifiedAddressInfo with parsed components
pub fn parse_unified_address(address: &str) -> AppResult<UnifiedAddressInfo> {
    OrchardAddressManager::parse_unified_address(address)
        .map_err(|e| AppError::ValidationError(format!("Invalid unified address: {}", e)))
}

/// Check if an address is a unified address, on any network
///
/// Mainnet UAs are `u1…`, testnet `utest1…`, regtest `uregtest1…`. Accepting only
/// `u1` made every unified address on a test chain look like a non-UA, so nothing
/// that branches on this could be exercised outside mainnet. Shared definition
/// with the transfer path (same prefix list) so the two cannot drift apart.
pub fn is_unified_address(address: &str) -> bool {
    crate::blockchain::zcash::orchard::transfer::is_unified_address(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_zcash_wallet() {
        let (address, private_key) = generate_zcash_wallet(NetworkType::Main).unwrap();

        assert!(address.starts_with("t1"));
        assert_eq!(private_key.len(), 64); // 32 bytes = 64 hex chars
        assert!(validate_zcash_address(&address));
    }

    #[test]
    fn test_generate_zcash_wallet_regtest() {
        // Regtest/testnet transparent addresses use the "tm" prefix
        let (address, _) = generate_zcash_wallet(NetworkType::Regtest).unwrap();
        assert!(address.starts_with("tm"), "regtest t-addr must start with tm, got {}", address);
        assert!(validate_zcash_address(&address));
    }

    #[test]
    fn test_import_zcash_wallet() {
        // Generate a wallet first
        let (original_address, private_key) = generate_zcash_wallet(NetworkType::Main).unwrap();

        // Import the same private key
        let imported_address = import_zcash_wallet(&private_key, NetworkType::Main).unwrap();

        assert_eq!(original_address, imported_address);
    }

    #[test]
    fn test_validate_zcash_address() {
        // Valid t1 address format (example)
        assert!(!validate_zcash_address("")); // Empty
        assert!(!validate_zcash_address("invalid")); // Random string

        // Generate and validate
        let (address, _) = generate_zcash_wallet(NetworkType::Main).unwrap();
        assert!(validate_zcash_address(&address));
    }
}
