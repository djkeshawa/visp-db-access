//! Fixture provider, deliberately absent from production builds.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use thiserror::Error;

use crate::{Provider, RegionError, RegionTest, ScanOutcome, SourceConfig, TestReport};

/// Fixture setup and parsing errors.
#[derive(Debug, Error)]
pub enum FakeError {
    /// A fixture path is required to explicitly enable the fake provider.
    #[error("VDA_DISCOVERY_FAKE_FIXTURE is not configured")]
    MissingPath,
    /// Reading the fixture failed.
    #[error("Unable to read discovery fixture")]
    Read(#[source] std::io::Error),
    /// The fixture is not a valid normalized scan outcome.
    #[error("Invalid discovery fixture")]
    Parse(#[source] serde_json::Error),
}

/// Deterministic local provider that reloads its JSON fixture on every call.
#[derive(Debug, Clone)]
pub struct FakeProvider {
    path: PathBuf,
}

impl FakeProvider {
    /// Explicitly enable a fixture provider from the process environment.
    pub fn from_env() -> Result<Self, FakeError> {
        let path = std::env::var_os("VDA_DISCOVERY_FAKE_FIXTURE")
            .filter(|path| !path.is_empty())
            .ok_or(FakeError::MissingPath)?;
        Ok(Self::from_path(PathBuf::from(path)))
    }

    /// Use a fixed fixture path (the contents are never cached).
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_owned(),
        }
    }

    async fn read(&self, source: &SourceConfig) -> Result<ScanOutcome, FakeError> {
        let contents = tokio::fs::read(&self.path).await.map_err(FakeError::Read)?;
        let mut outcome: ScanOutcome =
            serde_json::from_slice(&contents).map_err(FakeError::Parse)?;
        outcome
            .resources
            .retain(|resource| source.regions.contains(&resource.region));
        outcome.errors.retain(|error| {
            error
                .region
                .as_ref()
                .is_none_or(|region| source.regions.contains(region))
        });
        Ok(outcome)
    }
}

#[async_trait]
impl Provider for FakeProvider {
    async fn test(&self, source: &SourceConfig) -> TestReport {
        let outcome = self.read(source).await;
        let regions: Vec<_> = source
            .regions
            .iter()
            .map(|region| {
                let error = match &outcome {
                    Ok(outcome) => outcome
                        .errors
                        .iter()
                        .find(|error| {
                            error.region.is_none() || error.region.as_ref() == Some(region)
                        })
                        .map(|error| error.message.clone()),
                    Err(error) => Some(error.to_string()),
                };
                RegionTest {
                    region: region.clone(),
                    ok: error.is_none(),
                    error,
                }
            })
            .collect();
        let ok = outcome.is_ok() && regions.iter().all(|region| region.ok);
        TestReport {
            ok,
            account_id: outcome.is_ok().then(|| "123456789012".into()),
            identity_arn: outcome.is_ok().then(|| {
                "arn:aws:sts::123456789012:assumed-role/visp-db-access-fake/discovery".into()
            }),
            regions,
        }
    }

    async fn scan(&self, source: &SourceConfig) -> ScanOutcome {
        self.read(source).await.unwrap_or_else(|error| ScanOutcome {
            errors: vec![RegionError {
                region: None,
                message: error.to_string(),
            }],
            ..ScanOutcome::default()
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "vda-discovery-fixture-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn source() -> SourceConfig {
        SourceConfig {
            role_arn: None,
            external_id: None,
            regions: vec!["us-east-1".into()],
        }
    }

    #[tokio::test]
    async fn fixture_is_reloaded_and_errors_filtered_to_requested_regions() {
        let path = fixture_path();
        tokio::fs::write(&path, br#"{"resources":[],"errors":[{"region":"eu-west-1","message":"ignored outside source"}]}"#).await.unwrap();
        let provider = FakeProvider::from_path(&path);
        let source = source();
        assert!(provider.scan(&source).await.errors.is_empty());
        assert!(provider.test(&source).await.ok);
        tokio::fs::write(
            &path,
            br#"{"resources":[],"errors":[{"region":"us-east-1","message":"fixture changed"}]}"#,
        )
        .await
        .unwrap();
        let outcome = provider.scan(&source).await;
        assert_eq!(outcome.errors.first().unwrap().message, "fixture changed");
        let report = provider.test(&source).await;
        assert!(!report.ok);
        assert_eq!(
            report.identity_arn.as_deref(),
            Some("arn:aws:sts::123456789012:assumed-role/visp-db-access-fake/discovery")
        );
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn missing_or_malformed_fixture_is_a_source_wide_failure() {
        let path = fixture_path();
        let provider = FakeProvider::from_path(&path);
        let source = source();
        assert!(!provider.test(&source).await.ok);
        let outcome = provider.scan(&source).await;
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors.first().unwrap().region.is_none());
        tokio::fs::write(&path, b"invalid secret fixture content")
            .await
            .unwrap();
        let outcome = provider.scan(&source).await;
        assert_eq!(
            outcome.errors.first().unwrap().message,
            "Invalid discovery fixture"
        );
        tokio::fs::remove_file(path).await.unwrap();
    }
}
