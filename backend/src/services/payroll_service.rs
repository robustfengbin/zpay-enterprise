//! M1 F3.1 — Payroll Run service.
//!
//! Lifecycle: create → (optional) approve → execute → completed/partial/failed.
//! Execute walks items one-by-one calling `chain_client.transfer_native()` so
//! the route reuses every UTXO-selection / signing / broadcast path that the
//! single-transfer service already exercises. A future M2 path will compose
//! a single multi-output Orchard transaction via librustzcash, but doing so
//! requires the spending-key / Halo 2 proof plumbing that is not on M1
//! scope; until then the per-item path is the *real* on-chain flow.

use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use rust_decimal::Decimal;
use serde_json::json;

use crate::blockchain::{ChainRegistry, TransferParams};
use crate::db::models_m1::{
    CreatePayrollRunRequest, CreatePayrollRunResponse, PayrollItem, PayrollRun, PayrollRunSummary,
    ValidationError,
};
use crate::db::repositories::{
    ApprovalPolicyRepository, EmployeeRepository, PayrollRepository, WalletRepository,
};
use crate::error::{AppError, AppResult};
use crate::services::WalletService;

pub struct PayrollService {
    repo: PayrollRepository,
    employee_repo: EmployeeRepository,
    wallet_repo: WalletRepository,
    wallet_service: Arc<WalletService>,
    chain_registry: Arc<ChainRegistry>,
    policy_repo: ApprovalPolicyRepository,
}

impl PayrollService {
    pub fn new(
        repo: PayrollRepository,
        employee_repo: EmployeeRepository,
        wallet_repo: WalletRepository,
        wallet_service: Arc<WalletService>,
        chain_registry: Arc<ChainRegistry>,
        policy_repo: ApprovalPolicyRepository,
    ) -> Self {
        Self {
            repo,
            employee_repo,
            wallet_repo,
            wallet_service,
            chain_registry,
            policy_repo,
        }
    }

    // -----------------------------------------------------------------------
    // create_run — validate every item, accumulate total, insert atomically
    // -----------------------------------------------------------------------

    pub async fn create_run(
        &self,
        req: CreatePayrollRunRequest,
        user_id: i32,
    ) -> AppResult<CreatePayrollRunResponse> {
        let pay_period = req.pay_period.trim();
        if pay_period.is_empty() {
            return Err(AppError::ValidationError(
                "pay_period cannot be empty".to_string(),
            ));
        }
        if req.items.is_empty() {
            return Err(AppError::ValidationError(
                "items cannot be empty".to_string(),
            ));
        }

        // Resolve source wallet so we know which chain to validate addresses for.
        let wallet = self
            .wallet_repo
            .find_by_id(req.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "source_wallet_id {} not found",
                    req.source_wallet_id
                ))
            })?;
        let chain_client = self.chain_registry.get(&wallet.chain)?;

        // Validate every item up-front. We collect errors instead of bailing
        // on the first one so the operator can fix a whole CSV at once.
        let mut validation_errors: Vec<ValidationError> = Vec::new();
        let mut resolved: Vec<(Option<i32>, &crate::db::models_m1::PayrollItemInput, Decimal)> =
            Vec::with_capacity(req.items.len());
        let mut total = Decimal::ZERO;

        for (idx, input) in req.items.iter().enumerate() {
            let amount = match Decimal::from_str(&input.amount) {
                Ok(v) => v,
                Err(e) => {
                    validation_errors.push(ValidationError {
                        row_index: idx,
                        field: "amount".to_string(),
                        message: format!("invalid amount: {}", e),
                    });
                    continue;
                }
            };
            if amount <= Decimal::ZERO {
                validation_errors.push(ValidationError {
                    row_index: idx,
                    field: "amount".to_string(),
                    message: "amount must be positive".to_string(),
                });
                continue;
            }

            if !chain_client.validate_address(&input.employee_address) {
                validation_errors.push(ValidationError {
                    row_index: idx,
                    field: "employee_address".to_string(),
                    message: format!("invalid {} address", wallet.chain),
                });
                continue;
            }

            let employee_id = if let Some(code) = input.employee_code.as_deref() {
                match self.employee_repo.find_by_code(code).await? {
                    Some(emp) => Some(emp.id),
                    None => {
                        validation_errors.push(ValidationError {
                            row_index: idx,
                            field: "employee_code".to_string(),
                            message: format!("employee_code '{}' not found", code),
                        });
                        continue;
                    }
                }
            } else {
                None
            };

            total += amount;
            resolved.push((employee_id, input, amount));
        }

        if !validation_errors.is_empty() {
            return Ok(CreatePayrollRunResponse {
                run_id: 0,
                item_count: 0,
                validation_errors,
            });
        }

        let run_id = self
            .repo
            .create_run_with_items(
                pay_period,
                req.source_wallet_id,
                total,
                user_id,
                req.notes.as_deref(),
                &resolved,
            )
            .await?;

        Ok(CreatePayrollRunResponse {
            run_id,
            item_count: resolved.len() as i32,
            validation_errors: Vec::new(),
        })
    }

    // -----------------------------------------------------------------------
    // Reads
    // -----------------------------------------------------------------------

    pub async fn get_run_summary(&self, run_id: i32) -> AppResult<PayrollRunSummary> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("payroll_run {} not found", run_id)))?;
        let items = self.repo.list_items_by_run(run_id).await?;
        Ok(PayrollRunSummary { run, items })
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<PayrollRun>> {
        self.repo.list_runs(limit.clamp(1, 200), offset.max(0)).await
    }

    // -----------------------------------------------------------------------
    // execute_run — main fan-out path. Returns the post-execute summary so
    // the handler can render terminal state in one round-trip.
    // -----------------------------------------------------------------------

    pub async fn execute_run(&self, run_id: i32, user_id: i32) -> AppResult<ExecuteRunOutcome> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("payroll_run {} not found", run_id)))?;

        match run.status.as_str() {
            "pending" | "approved" => {}
            "awaiting_approval" => {
                return Err(AppError::ValidationError(
                    "payroll_run is awaiting approval and cannot be executed".to_string(),
                ));
            }
            other => {
                return Err(AppError::ValidationError(format!(
                    "payroll_run is in state '{}' and cannot be executed",
                    other
                )));
            }
        }

        let wallet = self
            .wallet_repo
            .find_by_id(run.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!(
                    "source_wallet {} vanished after run was created",
                    run.source_wallet_id
                ))
            })?;
        let chain_client = self.chain_registry.get(&wallet.chain)?;
        let native_symbol = chain_client.native_token_symbol().to_string();

        // F2.1 hook — gate large runs behind an approval policy.  We check on
        // each execute call so a policy added *after* the run was created
        // still applies.  Only `pending` runs may pivot; an already-approved
        // run skips the check (the approval is the explicit override).
        if run.status == "pending" {
            let matching = self
                .policy_repo
                .find_matching(&wallet.chain, &native_symbol, wallet.id, user_id)
                .await?
                .into_iter()
                .find(|p| run.total_amount >= p.amount_threshold);

            if let Some(policy) = matching {
                tracing::info!(
                    "[payroll] run {} total {} matched policy {} (threshold {}) -> awaiting_approval",
                    run.id, run.total_amount, policy.id, policy.amount_threshold
                );
                self.repo
                    .update_run_status(run.id, "awaiting_approval", None, None)
                    .await?;
                return Ok(ExecuteRunOutcome::AwaitingApproval {
                    run_id: run.id,
                    policy_id: policy.id,
                    threshold: policy.amount_threshold,
                });
            }
        }

        // Flip to `executing` before any RPC call so a crash mid-run leaves
        // a visible non-terminal state instead of silently looking pending.
        self.repo
            .update_run_status(run.id, "executing", None, None)
            .await?;

        let private_key = self.wallet_service.get_private_key(wallet.id).await?;
        let items = self.repo.list_items_by_run(run.id).await?;

        let mut submitted = 0u32;
        let mut failed = 0u32;

        for item in items.iter().filter(|i| i.status == "pending") {
            let params = TransferParams {
                from_address: wallet.address.clone(),
                to_address: item.employee_address.clone(),
                private_key: private_key.clone(),
                token: native_symbol.clone(),
                amount: item.amount,
                gas_price_gwei: None,
                gas_limit: None,
            };

            match chain_client.transfer_native(&params).await {
                Ok(tx_hash) => {
                    self.repo
                        .mark_item_submitted(item.id, &tx_hash, None)
                        .await?;
                    submitted += 1;
                    tracing::info!(
                        "[payroll] run {} item {} submitted: tx={}",
                        run.id, item.id, tx_hash
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.repo.mark_item_failed(item.id, &msg).await?;
                    failed += 1;
                    tracing::warn!(
                        "[payroll] run {} item {} failed: {}",
                        run.id, item.id, msg
                    );
                }
            }
        }

        let counts = self.repo.count_items_by_status(run.id).await?;
        let final_status = if counts.failed == 0 && counts.pending == 0 {
            "completed"
        } else if counts.submitted > 0 {
            "partial"
        } else {
            "failed"
        };
        self.repo
            .update_run_status(run.id, final_status, Some(user_id), Some(Utc::now()))
            .await?;

        Ok(ExecuteRunOutcome::Executed {
            run_id: run.id,
            submitted,
            failed,
            final_status: final_status.to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // cancel_run — only legal from `pending`. Anything that has touched the
    // chain (executing / partial / completed) must not be retroactively
    // canceled; the operator should retry individual failed items instead.
    // -----------------------------------------------------------------------

    pub async fn cancel_run(&self, run_id: i32) -> AppResult<PayrollRun> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("payroll_run {} not found", run_id)))?;
        if run.status != "pending" && run.status != "awaiting_approval" {
            return Err(AppError::ValidationError(format!(
                "payroll_run in state '{}' cannot be canceled",
                run.status
            )));
        }
        self.repo
            .update_run_status(run_id, "canceled", None, None)
            .await?;
        self.repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::InternalError("run vanished after cancel".to_string()))
    }

    // -----------------------------------------------------------------------
    // retry_item — replay a single failed item against the chain.
    // -----------------------------------------------------------------------

    pub async fn retry_item(&self, run_id: i32, item_id: i32) -> AppResult<PayrollItem> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("payroll_run {} not found", run_id)))?;
        let item = self
            .repo
            .find_item(run_id, item_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("payroll_item {} not found in run {}", item_id, run_id))
            })?;
        if item.status != "failed" {
            return Err(AppError::ValidationError(format!(
                "payroll_item is in state '{}'; only 'failed' items may be retried",
                item.status
            )));
        }

        let wallet = self
            .wallet_repo
            .find_by_id(run.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!(
                    "source_wallet {} vanished",
                    run.source_wallet_id
                ))
            })?;
        let chain_client = self.chain_registry.get(&wallet.chain)?;
        let private_key = self.wallet_service.get_private_key(wallet.id).await?;

        let params = TransferParams {
            from_address: wallet.address.clone(),
            to_address: item.employee_address.clone(),
            private_key,
            token: chain_client.native_token_symbol().to_string(),
            amount: item.amount,
            gas_price_gwei: None,
            gas_limit: None,
        };

        match chain_client.transfer_native(&params).await {
            Ok(tx_hash) => {
                self.repo
                    .mark_item_submitted(item.id, &tx_hash, None)
                    .await?;
            }
            Err(e) => {
                self.repo.mark_item_failed(item.id, &e.to_string()).await?;
                return Err(e);
            }
        }

        self.repo
            .find_item(run_id, item_id)
            .await?
            .ok_or_else(|| AppError::InternalError("item vanished after retry".to_string()))
    }

    // -----------------------------------------------------------------------
    // run_report — summary + per-item rows + aggregate counts
    // -----------------------------------------------------------------------

    pub async fn run_report(&self, run_id: i32) -> AppResult<serde_json::Value> {
        let summary = self.get_run_summary(run_id).await?;
        let counts = self.repo.count_items_by_status(run_id).await?;
        Ok(json!({
            "run": summary.run,
            "items": summary.items,
            "counts": {
                "pending": counts.pending,
                "submitted": counts.submitted,
                "failed": counts.failed,
                "total": counts.total,
            }
        }))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ExecuteRunOutcome {
    AwaitingApproval {
        run_id: i32,
        policy_id: i32,
        threshold: Decimal,
    },
    Executed {
        run_id: i32,
        submitted: u32,
        failed: u32,
        final_status: String,
    },
}
