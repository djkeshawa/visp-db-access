//! Read-only cloud inventory, independent of persistence and database credentials.

mod aws;
#[cfg(feature = "fake")]
mod fake;
pub mod mapping;

pub use aws::AwsProvider;
#[cfg(feature = "fake")]
pub use fake::{FakeError, FakeProvider};
pub use mapping::suggested_environment;

use std::collections::BTreeMap;

use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// Source-specific identity settings; no AWS access keys are stored here.
#[derive(Clone, Debug)]
pub struct SourceConfig {
    /// Cross-account IAM role, or ambient credentials when absent.
    pub role_arn: Option<String>,
    /// Optional confused-deputy protection for AssumeRole.
    pub external_id: Option<SecretString>,
    /// Validated AWS regions to inspect.
    pub regions: Vec<String>,
}

/// A normalized cloud database endpoint, before import or persistence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredDb {
    /// `rds_instance` or `aurora_cluster`.
    pub kind: String,
    /// Stable AWS resource identity.
    pub arn: String,
    /// Human-readable AWS identifier.
    pub identifier: String,
    /// AWS account extracted from the ARN.
    pub account_id: String,
    /// AWS region.
    pub region: String,
    /// Supported connection engine (`postgres` or `mysql`).
    pub engine: String,
    /// Original AWS engine name.
    pub engine_detail: String,
    /// Reported engine version.
    pub engine_version: String,
    /// Writer endpoint.
    pub host: String,
    /// Writer port.
    pub port: u16,
    /// First deterministic replica or Aurora reader endpoint.
    pub replica_host: Option<String>,
    /// Replica port.
    pub replica_port: Option<u16>,
    /// Initial database name, if AWS reports one.
    pub database: Option<String>,
    /// AWS lifecycle state.
    pub status_detail: String,
    /// AWS public accessibility flag.
    pub publicly_accessible: bool,
    /// Storage encryption flag.
    pub encrypted: bool,
    /// Multi-AZ flag.
    pub multi_az: bool,
    /// IAM database authentication flag.
    pub iam_auth_enabled: bool,
    /// VPC identifier, if reported.
    pub vpc_id: Option<String>,
    /// Resource tags, ordered for deterministic processing.
    pub tags: BTreeMap<String, String>,
    /// Safe environment default; callers may apply their configured tag keys.
    pub suggested_environment: String,
}

/// Failure confined to a region, or a source-wide identity failure (`None`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegionError {
    /// Region that failed; absent for fixture or identity errors.
    pub region: Option<String>,
    /// Sanitized upstream error description (never a credential).
    pub message: String,
}

/// A scan can retain successful inventory when another region fails.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScanOutcome {
    /// Supported, connectable cloud resources.
    pub resources: Vec<DiscoveredDb>,
    /// Failures whose regions must never be treated as empty inventories.
    #[serde(default)]
    pub errors: Vec<RegionError>,
    /// Unsupported or incomplete records omitted from the inventory.
    #[serde(default)]
    pub skipped: usize,
}

/// Per-region permission and reachability check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionTest {
    /// Inspected region.
    pub region: String,
    /// Whether a read-only DescribeDBInstances call succeeded.
    pub ok: bool,
    /// Safe error description on failure.
    pub error: Option<String>,
}

/// Identity plus regional read permissions; matches the discovery test API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestReport {
    /// Identity resolved and every region passed.
    pub ok: bool,
    /// STS caller account.
    pub account_id: Option<String>,
    /// STS caller identity ARN.
    pub identity_arn: Option<String>,
    /// Read permissions per requested region.
    pub regions: Vec<RegionTest>,
}

/// Object-safe interface used by the server and injected test providers.
#[async_trait]
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// Check caller identity and regional read access without importing anything.
    async fn test(&self, source: &SourceConfig) -> TestReport;
    /// Read inventory; failures are represented in the returned outcome.
    async fn scan(&self, source: &SourceConfig) -> ScanOutcome;
}
