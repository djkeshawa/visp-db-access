//! Environment defaults and validation for the complete policy wire format.
use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Cluster execution and access policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub max_rows: u64,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub max_cost: Option<f64>,
    pub max_concurrent_queries: u32,
    pub allow_writes: bool,
    pub require_approval_for_writes: bool,
    pub max_affected_rows: u64,
    pub allow_ddl: bool,
    pub route_reads_to_replica: bool,
    pub masked_columns: Vec<String>,
    pub blocked_tables: Vec<String>,
    pub allowed_cidrs: Vec<String>,
}
impl Policy {
    /// Conservative production defaults; larger read limits outside production.
    pub fn for_environment(environment: &str) -> Self {
        let prod = environment == "production";
        Self {
            max_rows: if prod { 1000 } else { 5000 },
            statement_timeout_ms: if prod { 15000 } else { 60000 },
            lock_timeout_ms: 2000,
            max_cost: None,
            max_concurrent_queries: 4,
            allow_writes: !prod,
            require_approval_for_writes: true,
            max_affected_rows: 1000,
            allow_ddl: false,
            route_reads_to_replica: true,
            masked_columns: vec![],
            blocked_tables: vec![],
            allowed_cidrs: vec![],
        }
    }
    /// Reject unbounded resource limits and malformed patterns/CIDRs.
    pub fn validate(&self) -> Result<(), ApiError> {
        if !(1..=100000).contains(&self.max_rows) || !(1..=100000).contains(&self.max_affected_rows)
        {
            return Err(ApiError::validation("Row limits must be 1..=100000"));
        }
        if !(100..=600000).contains(&self.statement_timeout_ms)
            || !(100..=600000).contains(&self.lock_timeout_ms)
            || self.lock_timeout_ms > self.statement_timeout_ms
        {
            return Err(ApiError::validation(
                "Timeouts must be 100..=600000ms and lock timeout cannot exceed statement timeout",
            ));
        }
        if !(1..=64).contains(&self.max_concurrent_queries) {
            return Err(ApiError::validation("Concurrency must be 1..=64"));
        }
        if self.max_cost.is_some_and(|v| !v.is_finite() || v <= 0.) {
            return Err(ApiError::validation("Cost must be finite and positive"));
        }
        for patterns in [&self.masked_columns, &self.blocked_tables] {
            if patterns.len() > 128 || patterns.iter().any(|p| p.is_empty() || p.len() > 256) {
                return Err(ApiError::validation("Invalid policy patterns"));
            }
        }
        crate::network::parse_cidrs(&self.allowed_cidrs)?;
        Ok(())
    }
    /// Project fields needed by the pure SQL guard.
    pub fn guard(&self) -> vda_guard::GuardPolicy {
        vda_guard::GuardPolicy {
            max_rows: self.max_rows,
            allow_writes: self.allow_writes,
            require_approval_for_writes: self.require_approval_for_writes,
            allow_ddl: self.allow_ddl,
            blocked_tables: self.blocked_tables.clone(),
        }
    }
    /// Runtime transaction limits, including the independent byte cap.
    pub fn limits(&self) -> vda_connectors::ExecLimits {
        vda_connectors::ExecLimits {
            statement_timeout: Duration::from_millis(self.statement_timeout_ms),
            lock_timeout: Duration::from_millis(self.lock_timeout_ms),
            max_rows: self.max_rows as usize,
            max_bytes: 32 * 1024 * 1024,
        }
    }
}
