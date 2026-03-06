use sqlx::PgPool;

/// Dual-pool abstraction for read-write separation.
/// If no replica is configured, falls back to primary for all operations.
#[derive(Clone, Debug)]
pub struct DbPools {
    primary: PgPool,
    replica: PgPool,
}

impl DbPools {
    pub fn new(primary: PgPool, replica: Option<PgPool>) -> Self {
        let replica = replica.unwrap_or_else(|| primary.clone());
        Self { primary, replica }
    }

    /// Read pool — for queries that tolerate replication lag.
    pub fn read(&self) -> &PgPool {
        &self.replica
    }

    /// Write pool — for mutations and lag-sensitive reads.
    pub fn write(&self) -> &PgPool {
        &self.primary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn create_test_pool(url: &str) -> PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy(url)
            .expect("Failed to create lazy pool")
    }

    #[tokio::test]
    async fn test_new_with_replica() {
        let primary = create_test_pool("postgres://user:pass@primary:5432/db");
        let replica = create_test_pool("postgres://user:pass@replica:5433/db");

        let pools = DbPools::new(primary, Some(replica));

        let _ = pools.read();
        let _ = pools.write();
    }

    #[tokio::test]
    async fn test_new_without_replica_falls_back_to_primary() {
        let primary = create_test_pool("postgres://user:pass@primary:5432/db");

        let pools = DbPools::new(primary, None);

        let _ = pools.read();
        let _ = pools.write();
    }

    #[tokio::test]
    async fn test_clone() {
        let primary = create_test_pool("postgres://user:pass@primary:5432/db");
        let pools = DbPools::new(primary, None);

        let cloned = pools.clone();
        let _ = cloned.read();
        let _ = cloned.write();
    }
}
