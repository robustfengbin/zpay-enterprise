use crate::db::models::Wallet;
use crate::error::AppResult;
use sqlx::MySqlPool;

#[derive(Clone)]
pub struct WalletRepository {
    pool: MySqlPool,
}

impl WalletRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        name: &str,
        address: &str,
        encrypted_private_key: &str,
        chain: &str,
        orchard_birthday_height: Option<u64>,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO wallets (name, address, encrypted_private_key, chain, orchard_birthday_height) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(name)
        .bind(address)
        .bind(encrypted_private_key)
        .bind(chain)
        .bind(orchard_birthday_height)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_id() as i32)
    }

    #[allow(dead_code)]
    pub async fn update_birthday_height(&self, id: i32, birthday_height: u64) -> AppResult<()> {
        sqlx::query("UPDATE wallets SET orchard_birthday_height = ? WHERE id = ?")
            .bind(birthday_height)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<Wallet>> {
        let wallet = sqlx::query_as::<_, Wallet>(
            "SELECT id, name, address, encrypted_private_key, chain, is_active, created_at, orchard_birthday_height FROM wallets WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(wallet)
    }

    pub async fn find_by_address(&self, address: &str, chain: &str) -> AppResult<Option<Wallet>> {
        let wallet = sqlx::query_as::<_, Wallet>(
            "SELECT id, name, address, encrypted_private_key, chain, is_active, created_at, orchard_birthday_height FROM wallets WHERE address = ? AND chain = ?"
        )
        .bind(address)
        .bind(chain)
        .fetch_optional(&self.pool)
        .await?;

        Ok(wallet)
    }

    pub async fn list_all(&self) -> AppResult<Vec<Wallet>> {
        let wallets = sqlx::query_as::<_, Wallet>(
            "SELECT id, name, address, encrypted_private_key, chain, is_active, created_at, orchard_birthday_height FROM wallets ORDER BY id"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(wallets)
    }

    pub async fn list_by_chain(&self, chain: &str) -> AppResult<Vec<Wallet>> {
        let wallets = sqlx::query_as::<_, Wallet>(
            "SELECT id, name, address, encrypted_private_key, chain, is_active, created_at, orchard_birthday_height FROM wallets WHERE chain = ? ORDER BY id"
        )
        .bind(chain)
        .fetch_all(&self.pool)
        .await?;

        Ok(wallets)
    }

    pub async fn get_active_wallet(&self, chain: &str) -> AppResult<Option<Wallet>> {
        let wallet = sqlx::query_as::<_, Wallet>(
            "SELECT id, name, address, encrypted_private_key, chain, is_active, created_at, orchard_birthday_height FROM wallets WHERE chain = ? AND is_active = TRUE LIMIT 1"
        )
        .bind(chain)
        .fetch_optional(&self.pool)
        .await?;

        Ok(wallet)
    }

    pub async fn set_active(&self, id: i32, chain: &str) -> AppResult<()> {
        // First, deactivate all wallets in the same chain
        sqlx::query("UPDATE wallets SET is_active = FALSE WHERE chain = ?")
            .bind(chain)
            .execute(&self.pool)
            .await?;

        // Then, activate the specified wallet
        sqlx::query("UPDATE wallets SET is_active = TRUE WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete(&self, id: i32) -> AppResult<()> {
        sqlx::query("DELETE FROM wallets WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
