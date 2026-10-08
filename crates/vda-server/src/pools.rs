//! Idle-evicting target pool cache and per-cluster concurrency limits.
use crate::{app::AppState, db::ClusterRecord, error::ApiError, policy::Policy};
use moka::future::Cache;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use uuid::Uuid;
use vda_connectors::{ConnectionSpec, Engine, PoolOptions, TargetPool, TlsMode};
/// Primary and optional replica pools sharing a concurrency gate.
#[derive(Debug)]
pub struct ClusterPools {
    pub primary: TargetPool,
    pub replica: Option<TargetPool>,
    pub semaphore: Arc<Semaphore>,
}
/// Per-node pool cache. Version keys prevent stale configuration across nodes.
#[derive(Debug, Clone)]
pub struct Pools {
    cache: Cache<(Uuid, chrono::DateTime<chrono::Utc>), Arc<ClusterPools>>,
}
impl Default for Pools {
    fn default() -> Self {
        Self::new()
    }
}
impl Pools {
    /// Bound the number and idle lifetime of target pools.
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(512)
                .time_to_idle(Duration::from_secs(300))
                .build(),
        }
    }
    /// Load a versioned pool bundle, coalescing concurrent creation.
    pub async fn get(
        &self,
        state: &AppState,
        cluster: &ClusterRecord,
        policy: &Policy,
    ) -> Result<Arc<ClusterPools>, ApiError> {
        let key = (cluster.id, cluster.updated_at);
        let value = self
            .cache
            .try_get_with(key, async {
                let primary = TargetPool::new_lazy(
                    &connection_spec(state, cluster, false).await?,
                    PoolOptions::default(),
                )?;
                let replica = if cluster.replica_host.is_some() {
                    Some(TargetPool::new_lazy(
                        &connection_spec(state, cluster, true).await?,
                        PoolOptions::default(),
                    )?)
                } else {
                    None
                };
                Ok::<_, ApiError>(Arc::new(ClusterPools {
                    primary,
                    replica,
                    semaphore: Arc::new(Semaphore::new(policy.max_concurrent_queries as usize)),
                }))
            })
            .await
            .map_err(|e| (*e).clone())?;
        metrics::gauge!("vda_target_pool_bundles").set(self.cache.entry_count() as f64);
        pool_metrics(cluster.id, "primary", &value.primary);
        if let Some(replica) = &value.replica {
            pool_metrics(cluster.id, "replica", replica);
        }
        metrics::gauge!("vda_cluster_available_permits", "cluster"=>cluster.id.to_string())
            .set(value.semaphore.available_permits() as f64);

        Ok(value)
    }
    /// Invalidate every version for a cluster following a local update.
    pub async fn invalidate(&self, id: Uuid) {
        for (key, _) in self.cache.iter() {
            if key.0 == id {
                self.cache.invalidate(&*key).await;
            }
        }
    }
    /// Close all currently cached target pools during graceful shutdown.
    pub async fn close(&self) {
        for (_, pools) in self.cache.iter() {
            pools.primary.close().await;
            if let Some(replica) = &pools.replica {
                replica.close().await;
            }
        }
        self.cache.invalidate_all();
        self.cache.run_pending_tasks().await;
    }
}
/// Decrypt and validate credentials immediately before opening a pool.
pub async fn connection_spec(
    state: &AppState,
    cluster: &ClusterRecord,
    replica: bool,
) -> Result<ConnectionSpec, ApiError> {
    let host = if replica {
        cluster
            .replica_host
            .clone()
            .ok_or_else(ApiError::not_found)?
    } else {
        cluster.host.clone()
    };
    let port = u16::try_from(if replica {
        cluster.replica_port.unwrap_or(cluster.port)
    } else {
        cluster.port
    })
    .map_err(|_| ApiError::internal())?;
    crate::network::validate_host(&host, port, state.config.allow_private_targets).await?;
    let engine = match cluster.engine.as_str() {
        "postgres" => Engine::Postgres,
        "mysql" => Engine::Mysql,
        _ => return Err(ApiError::internal()),
    };
    let tls = match cluster.tls_mode.as_str() {
        "disable" => TlsMode::Disable,
        "prefer" => TlsMode::Prefer,
        "require" => TlsMode::Require,
        "verify_full" => TlsMode::VerifyFull,
        _ => return Err(ApiError::internal()),
    };
    Ok(ConnectionSpec {
        engine,
        host,
        port,
        database: cluster.database.clone(),
        username: cluster.username.clone(),
        password: state
            .crypto
            .decrypt(cluster.id, &cluster.password_enc)
            .map_err(|_| ApiError::credentials_unreadable(cluster.id))?
            .into(),
        tls,
        ca_cert_pem: None,
        application_name: "visp-db-access".into(),
    })
}

fn pool_metrics(cluster: Uuid, endpoint: &'static str, pool: &TargetPool) {
    let (total, idle) = match pool {
        TargetPool::Postgres(pool) => (pool.size(), pool.num_idle()),
        TargetPool::Mysql(pool) => (pool.size(), pool.num_idle()),
    };
    metrics::gauge!("vda_target_pool_connections", "cluster"=>cluster.to_string(), "endpoint"=>endpoint).set(total as f64);
    metrics::gauge!("vda_target_pool_idle_connections", "cluster"=>cluster.to_string(), "endpoint"=>endpoint).set(idle as f64);
}

/// Revalidate primary and replica DNS, including when pools are already cached.
pub async fn validate_endpoints(state: &AppState, cluster: &ClusterRecord) -> Result<(), ApiError> {
    let port = u16::try_from(cluster.port).map_err(|_| ApiError::internal())?;
    crate::network::validate_host(&cluster.host, port, state.config.allow_private_targets).await?;
    if let Some(host) = &cluster.replica_host {
        let port = u16::try_from(cluster.replica_port.unwrap_or(cluster.port))
            .map_err(|_| ApiError::internal())?;
        crate::network::validate_host(host, port, state.config.allow_private_targets).await?;
    }
    Ok(())
}
