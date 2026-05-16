use std::sync::Arc;

use crate::blockchain::ChainRegistry;
use crate::db::models_m1::{
    CreateEmployeeRequest, Employee, UpdateEmployeeRequest,
};
use crate::db::repositories::EmployeeRepository;
use crate::error::{AppError, AppResult};

pub struct EmployeeService {
    repo: EmployeeRepository,
    chain_registry: Arc<ChainRegistry>,
}

impl EmployeeService {
    pub fn new(repo: EmployeeRepository, chain_registry: Arc<ChainRegistry>) -> Self {
        Self {
            repo,
            chain_registry,
        }
    }

    pub async fn create(&self, req: CreateEmployeeRequest) -> AppResult<Employee> {
        // 1. employee_code uniqueness
        if let Some(_existing) = self.repo.find_by_code(&req.employee_code).await? {
            return Err(AppError::AlreadyExists(format!(
                "employee_code '{}' already exists",
                req.employee_code
            )));
        }

        // 2. chain must be a registered chain
        let chain_client = self.chain_registry.get(&req.chain)?;

        // 3. address must be valid for that chain
        if !chain_client.validate_address(&req.wallet_address) {
            return Err(AppError::ValidationError(format!(
                "invalid {} address: {}",
                req.chain, req.wallet_address
            )));
        }

        // 4. trim name (defensive — empty name would be a footgun in reports)
        let name = req.name.trim();
        if name.is_empty() {
            return Err(AppError::ValidationError(
                "employee name cannot be empty".to_string(),
            ));
        }

        let id = self
            .repo
            .create(
                &req.employee_code,
                name,
                &req.wallet_address,
                &req.chain,
                req.tags.as_ref(),
            )
            .await?;

        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::InternalError("employee vanished after insert".to_string()))
    }

    pub async fn get(&self, id: i32) -> AppResult<Employee> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("employee {} not found", id)))
    }

    pub async fn list(&self, active_only: bool) -> AppResult<Vec<Employee>> {
        self.repo.list(active_only).await
    }

    pub async fn update(&self, id: i32, req: UpdateEmployeeRequest) -> AppResult<Employee> {
        // Ensure the row exists first — otherwise UPDATE silently affects 0 rows
        // and the caller gets a confusing 200 with stale data.
        let existing = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("employee {} not found", id)))?;

        // If wallet_address changes, re-validate against the (unchanged) chain.
        if let Some(addr) = req.wallet_address.as_deref() {
            let chain_client = self.chain_registry.get(&existing.chain)?;
            if !chain_client.validate_address(addr) {
                return Err(AppError::ValidationError(format!(
                    "invalid {} address: {}",
                    existing.chain, addr
                )));
            }
        }

        if let Some(name) = req.name.as_deref() {
            if name.trim().is_empty() {
                return Err(AppError::ValidationError(
                    "employee name cannot be empty".to_string(),
                ));
            }
        }

        self.repo
            .update(
                id,
                req.name.as_deref(),
                req.wallet_address.as_deref(),
                req.tags.as_ref(),
                req.active,
            )
            .await?;

        self.get(id).await
    }

    pub async fn soft_delete(&self, id: i32) -> AppResult<()> {
        // 404 if not present, rather than silently no-op.
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("employee {} not found", id)))?;
        self.repo.soft_delete(id).await
    }
}
