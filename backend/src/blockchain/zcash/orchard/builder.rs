//! Orchard transaction builder
//!
//! Builds shielded transactions using the Orchard protocol with Halo 2 proofs.

#![allow(dead_code)]

use super::{
    constants::{DEFAULT_FEE_ZATOSHIS, GRACE_ACTIONS, MARGINAL_FEE_ZATOSHIS},
    keys::OrchardSpendingKey,
    scanner::OrchardNote,
    OrchardError, OrchardResult, ShieldedPool,
};
use serde::{Deserialize, Serialize};

/// Parameters for an Orchard (shielded) transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchardTransferParams {
    /// Wallet ID (primary identifier for notes)
    pub wallet_id: i32,

    /// Recipient address (unified or shielded)
    pub to_address: String,

    /// Amount in zatoshis
    pub amount_zatoshis: u64,

    /// Optional encrypted memo (512 bytes max)
    pub memo: Option<String>,

    /// Target pool for the transfer
    pub target_pool: ShieldedPool,

    /// Whether to include change in the same pool
    pub change_to_same_pool: bool,
}

/// A built Orchard action (spend + output)
#[derive(Debug, Clone)]
pub struct OrchardAction {
    /// Spend components
    pub spend: Option<OrchardSpend>,
    /// Output components
    pub output: Option<OrchardOutput>,
    /// Randomized verification key
    pub rk: [u8; 32],
    /// Commitment to the action
    pub cm: [u8; 32],
    /// Nullifier (if spending)
    pub nullifier: Option<[u8; 32]>,
    /// Encrypted note ciphertext
    pub encrypted_note: Vec<u8>,
}

/// Orchard spend information
#[derive(Debug, Clone)]
pub struct OrchardSpend {
    /// Note being spent
    pub note_commitment: [u8; 32],
    /// Nullifier for this spend
    pub nullifier: [u8; 32],
    /// Value in zatoshis
    pub value: u64,
    /// Merkle path for the note
    pub merkle_path: Vec<[u8; 32]>,
}

/// Orchard output information
#[derive(Debug, Clone)]
pub struct OrchardOutput {
    /// Recipient address (raw bytes)
    pub recipient: Vec<u8>,
    /// Value in zatoshis
    pub value: u64,
    /// Memo (encrypted)
    pub memo: [u8; 512],
    /// Note commitment
    pub note_commitment: [u8; 32],
}

/// Builder for Orchard transactions
pub struct OrchardTransactionBuilder {
    /// Notes available for spending
    spendable_notes: Vec<OrchardNote>,
    /// Actions to include in the transaction
    actions: Vec<OrchardAction>,
    /// Anchor height
    anchor_height: u64,
    /// Anchor commitment tree root
    anchor: [u8; 32],
    /// Transaction expiry height
    expiry_height: u32,
    /// Consensus branch ID
    consensus_branch_id: u32,
    /// Calculated fee
    fee_zatoshis: u64,
}

impl OrchardTransactionBuilder {
    /// Create a new Orchard transaction builder
    pub fn new(
        anchor_height: u64,
        anchor: [u8; 32],
        expiry_height: u32,
        consensus_branch_id: u32,
    ) -> Self {
        Self {
            spendable_notes: Vec::new(),
            actions: Vec::new(),
            anchor_height,
            anchor,
            expiry_height,
            consensus_branch_id,
            fee_zatoshis: DEFAULT_FEE_ZATOSHIS,
        }
    }

    /// Add spendable notes to the builder
    pub fn add_spendable_notes(&mut self, notes: Vec<OrchardNote>) {
        self.spendable_notes.extend(notes);
    }

    /// Get the total value of spendable notes
    pub fn spendable_value(&self) -> u64 {
        self.spendable_notes.iter().map(|n| n.value_zatoshis).sum()
    }

    /// Add a shielded output (recipient)
    pub fn add_output(
        &mut self,
        recipient: &str,
        amount_zatoshis: u64,
        memo: Option<&str>,
    ) -> OrchardResult<()> {
        // Parse recipient address
        let recipient_bytes = self.parse_recipient_address(recipient)?;

        // Prepare memo
        let mut memo_bytes = [0u8; 512];
        if let Some(m) = memo {
            let m_bytes = m.as_bytes();
            let len = m_bytes.len().min(512);
            memo_bytes[..len].copy_from_slice(&m_bytes[..len]);
        }

        // Generate randomness for the output
        let mut rng = rand::thread_rng();
        let mut rcm = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rng, &mut rcm);

        // Compute note commitment (simplified)
        let note_commitment = self.compute_note_commitment(&recipient_bytes, amount_zatoshis, &rcm)?;

        let output = OrchardOutput {
            recipient: recipient_bytes,
            value: amount_zatoshis,
            memo: memo_bytes,
            note_commitment,
        };

        // Create action with output only (no spend yet)
        let action = OrchardAction {
            spend: None,
            output: Some(output),
            rk: [0u8; 32],
            cm: note_commitment,
            nullifier: None,
            encrypted_note: Vec::new(),
        };

        self.actions.push(action);
        Ok(())
    }

    /// Calculate the required fee using ZIP 317 formula
    pub fn calculate_fee(&self) -> u64 {
        let num_actions = self.actions.len() as u32;

        if num_actions <= GRACE_ACTIONS {
            DEFAULT_FEE_ZATOSHIS
        } else {
            DEFAULT_FEE_ZATOSHIS + (num_actions - GRACE_ACTIONS) as u64 * MARGINAL_FEE_ZATOSHIS
        }
    }

    /// Build the transaction bundle
    ///
    /// This will:
    /// 1. Select notes to spend
    /// 2. Create spends and outputs
    /// 3. Balance the transaction
    /// 4. Generate proofs (expensive!)
    pub fn build(
        mut self,
        spending_key: &OrchardSpendingKey,
        _params: &OrchardTransferParams,
    ) -> OrchardResult<OrchardBundle> {
        // Calculate total output value
        let total_output: u64 = self.actions.iter().filter_map(|a| a.output.as_ref()).map(|o| o.value).sum();

        // Calculate fee
        self.fee_zatoshis = self.calculate_fee();
        let total_needed = total_output + self.fee_zatoshis;

        // Check if we have enough spendable value
        let total_spendable = self.spendable_value();
        if total_spendable < total_needed {
            return Err(OrchardError::InsufficientBalance {
                available: total_spendable,
                required: total_needed,
            });
        }

        // Select notes to spend
        let selected_notes = self.select_notes(total_needed)?;

        // Create spends for selected notes
        for note in &selected_notes {
            let spend = self.create_spend(note, spending_key)?;

            // Find an action without a spend or create a new one
            let action_idx = self.actions.iter().position(|a| a.spend.is_none());

            if let Some(idx) = action_idx {
                self.actions[idx].spend = Some(spend.clone());
                self.actions[idx].nullifier = Some(spend.nullifier);
            } else {
                // Create a dummy output for padding
                let action = OrchardAction {
                    spend: Some(spend.clone()),
                    output: None, // Will be filled with dummy
                    rk: [0u8; 32],
                    cm: [0u8; 32],
                    nullifier: Some(spend.nullifier),
                    encrypted_note: Vec::new(),
                };
                self.actions.push(action);
            }
        }

        // Calculate change
        let total_input: u64 = selected_notes.iter().map(|n| n.value_zatoshis).sum();
        let change = total_input - total_output - self.fee_zatoshis;

        // Add change output if needed
        if change > 0 {
            // Generate change address from spending key
            let change_address = self.derive_change_address(spending_key)?;
            self.add_output(&change_address, change, None)?;
        }

        // Pad actions to be even (Orchard requires this)
        self.pad_actions()?;

        // Generate proofs for all actions
        let proofs = self.generate_proofs(spending_key)?;

        // Compute binding signature before moving actions
        let binding_signature = self.compute_binding_signature()?;
        let value_balance = (total_input as i64) - (total_output as i64) - (self.fee_zatoshis as i64);

        // Build the bundle
        let bundle = OrchardBundle {
            actions: self.actions,
            anchor: self.anchor,
            proofs,
            binding_signature,
            flags: OrchardFlags {
                spends_enabled: true,
                outputs_enabled: true,
            },
            value_balance,
        };

        Ok(bundle)
    }

    /// Select notes to spend to cover the required amount
    fn select_notes(&self, amount_needed: u64) -> OrchardResult<Vec<OrchardNote>> {
        if self.spendable_notes.is_empty() {
            return Err(OrchardError::NoSpendableNotes);
        }

        let mut selected = Vec::new();
        let mut total_selected: u64 = 0;

        // Simple greedy selection - select notes until we have enough
        // In production, use a more sophisticated algorithm (e.g., minimize dust)
        let mut sorted_notes = self.spendable_notes.clone();
        sorted_notes.sort_by(|a, b| b.value_zatoshis.cmp(&a.value_zatoshis));

        for note in sorted_notes {
            if total_selected >= amount_needed {
                break;
            }
            total_selected += note.value_zatoshis;
            selected.push(note);
        }

        if total_selected < amount_needed {
            return Err(OrchardError::InsufficientBalance {
                available: total_selected,
                required: amount_needed,
            });
        }

        Ok(selected)
    }

    /// Create a spend from a note
    fn create_spend(
        &self,
        note: &OrchardNote,
        _spending_key: &OrchardSpendingKey,
    ) -> OrchardResult<OrchardSpend> {
        // Get merkle path for the note
        let merkle_path = note.merkle_path.clone().ok_or(OrchardError::WitnessNotFound)?;

        // Compute nullifier
        let nullifier = self.compute_nullifier(note)?;

        Ok(OrchardSpend {
            note_commitment: note.note_commitment,
            nullifier,
            value: note.value_zatoshis,
            merkle_path,
        })
    }

    /// Compute nullifier for a note
    fn compute_nullifier(&self, note: &OrchardNote) -> OrchardResult<[u8; 32]> {
        // In real implementation, use the spending key and note data
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashOrchardNf__")
            .to_state();
        hasher.update(&note.note_commitment);
        hasher.update(&note.position.to_le_bytes());
        let result = hasher.finalize();

        let mut nullifier = [0u8; 32];
        nullifier.copy_from_slice(result.as_bytes());
        Ok(nullifier)
    }

    /// Compute note commitment
    fn compute_note_commitment(
        &self,
        recipient: &[u8],
        value: u64,
        rcm: &[u8; 32],
    ) -> OrchardResult<[u8; 32]> {
        let mut hasher = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashOrchardCm__")
            .to_state();
        hasher.update(recipient);
        hasher.update(&value.to_le_bytes());
        hasher.update(rcm);
        let result = hasher.finalize();

        let mut commitment = [0u8; 32];
        commitment.copy_from_slice(result.as_bytes());
        Ok(commitment)
    }

    /// Parse recipient address to raw bytes
    fn parse_recipient_address(&self, address: &str) -> OrchardResult<Vec<u8>> {
        if super::transfer::is_unified_address(address) {
            // Unified address - extract Orchard receiver
            // In real implementation, decode the unified address
            Ok(address.as_bytes().to_vec())
        } else if address.starts_with("zs") {
            // Sapling address - not supported for Orchard output
            Err(OrchardError::AddressGeneration(
                "Sapling addresses not supported for Orchard transfers".to_string(),
            ))
        } else {
            Err(OrchardError::InvalidUnifiedAddress(format!(
                "Unsupported address format: {}",
                address
            )))
        }
    }

    /// Derive change address from spending key
    fn derive_change_address(&self, _spending_key: &OrchardSpendingKey) -> OrchardResult<String> {
        // In real implementation, derive a proper internal address
        // For now, return a placeholder
        Ok("u1change_placeholder".to_string())
    }

    /// Pad actions to be even (Orchard requirement)
    fn pad_actions(&mut self) -> OrchardResult<()> {
        if self.actions.len() % 2 == 1 {
            // Add a dummy action
            let dummy_action = OrchardAction {
                spend: None,
                output: None,
                rk: [0u8; 32],
                cm: [0u8; 32],
                nullifier: None,
                encrypted_note: Vec::new(),
            };
            self.actions.push(dummy_action);
        }
        Ok(())
    }

    /// Generate Halo 2 proofs for all actions
    ///
    /// This is the most computationally expensive part of building an Orchard transaction.
    /// In production, this should be done in a separate thread.
    fn generate_proofs(&self, _spending_key: &OrchardSpendingKey) -> OrchardResult<Vec<[u8; 2720]>> {
        // In real implementation:
        // 1. Create circuits for each action
        // 2. Generate Halo 2 proofs using the proving key
        // 3. This takes 2-5 seconds per action

        let proofs: Vec<[u8; 2720]> = self.actions.iter().map(|_| [0u8; 2720]).collect();

        Ok(proofs)
    }

    /// Compute binding signature
    fn compute_binding_signature(&self) -> OrchardResult<[u8; 64]> {
        // In real implementation, sign the transaction with the binding key
        Ok([0u8; 64])
    }
}

/// Orchard bundle flags
#[derive(Debug, Clone)]
pub struct OrchardFlags {
    pub spends_enabled: bool,
    pub outputs_enabled: bool,
}

/// Complete Orchard bundle ready for inclusion in a transaction
#[derive(Debug, Clone)]
pub struct OrchardBundle {
    /// All actions in the bundle
    pub actions: Vec<OrchardAction>,
    /// Anchor (commitment tree root)
    pub anchor: [u8; 32],
    /// Halo 2 proofs for each action
    pub proofs: Vec<[u8; 2720]>,
    /// Binding signature
    pub binding_signature: [u8; 64],
    /// Bundle flags
    pub flags: OrchardFlags,
    /// Value balance (positive = shielded to transparent, negative = transparent to shielded)
    pub value_balance: i64,
}

impl OrchardBundle {
    /// Serialize the bundle for inclusion in a transaction
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::new();

        // Number of actions (compactSize)
        result.push(self.actions.len() as u8);

        // Serialize each action
        for (action, proof) in self.actions.iter().zip(&self.proofs) {
            // Action data
            result.extend_from_slice(&action.cm);
            result.extend_from_slice(action.nullifier.as_ref().unwrap_or(&[0u8; 32]));
            result.extend_from_slice(&action.rk);
            result.extend_from_slice(&action.encrypted_note);

            // Proof
            result.extend_from_slice(proof);
        }

        // Anchor
        result.extend_from_slice(&self.anchor);

        // Value balance
        result.extend_from_slice(&self.value_balance.to_le_bytes());

        // Flags
        let flags_byte = (self.flags.spends_enabled as u8) | ((self.flags.outputs_enabled as u8) << 1);
        result.push(flags_byte);

        // Binding signature
        result.extend_from_slice(&self.binding_signature);

        result
    }

    /// Get the total number of actions
    pub fn num_actions(&self) -> usize {
        self.actions.len()
    }

    /// Get all nullifiers from spends
    pub fn nullifiers(&self) -> Vec<[u8; 32]> {
        self.actions
            .iter()
            .filter_map(|a| a.nullifier)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_calculation() {
        let builder = OrchardTransactionBuilder::new(
            2000000,
            [0u8; 32],
            2000100,
            0xc8e71055,
        );

        // Empty builder should have default fee
        assert_eq!(builder.calculate_fee(), DEFAULT_FEE_ZATOSHIS);
    }

    #[test]
    fn test_add_output() {
        let mut builder = OrchardTransactionBuilder::new(
            2000000,
            [0u8; 32],
            2000100,
            0xc8e71055,
        );

        // A unified address is now recognised by prefix *and* plausible length
        // (u1/utest1/uregtest1 — see transfer::is_unified_address), so the fixture
        // has to be UA-shaped rather than the old "u1testaddress" stub.
        let recipient = format!("u1{}", "q".repeat(120));
        builder
            .add_output(&recipient, 100000, Some("Test memo"))
            .unwrap();

        assert_eq!(builder.actions.len(), 1);
        assert!(builder.actions[0].output.is_some());
    }
}
