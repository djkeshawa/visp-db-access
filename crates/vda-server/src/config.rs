//! Validated CLI and environment configuration.
use clap::{Parser, Subcommand};
use ipnet::IpNet;
use secrecy::SecretString;
use std::net::SocketAddr;

/// Command-line entry point.
#[derive(Debug, Parser)]
#[command(name = "visp-db-access", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub config: Config,
}
/// Operational subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the gateway (default).
    Serve,
    /// Apply metadata migrations.
    Migrate,
    /// Create an organization administrator.
    CreateAdmin {
        #[arg(long)]
        email: String,
        #[arg(long)]
        name: String,
        #[arg(long, env = "VDA_ADMIN_PASSWORD", hide_env_values = true)]
        password: SecretString,
    },
    /// Generate a base64 master key without connecting to any database.
    GenKey,
}
/// Gateway configuration. Secrets are redacted by their types.
#[derive(Debug, Clone, clap::Args)]
pub struct Config {
    #[arg(long, env = "VDA_DATABASE_URL", hide_env_values = true)]
    pub database_url: Option<SecretString>,
    #[arg(long, env = "VDA_MASTER_KEY", hide_env_values = true)]
    pub master_key: Option<SecretString>,
    #[arg(long, env = "VDA_BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,
    #[arg(long, env = "VDA_METADATA_POOL_SIZE", default_value = "20")]
    pub metadata_pool_size: u32,
    #[arg(long, env = "VDA_BOOTSTRAP_ADMIN_EMAIL")]
    pub bootstrap_admin_email: Option<String>,
    #[arg(long, env = "VDA_BOOTSTRAP_ADMIN_PASSWORD", hide_env_values = true)]
    pub bootstrap_admin_password: Option<SecretString>,
    #[arg(long, env="VDA_COOKIE_SECURE", default_value="true", action=clap::ArgAction::Set)]
    pub cookie_secure: bool,
    #[arg(long, env = "VDA_ALLOWED_CIDRS", default_value = "")]
    pub allowed_cidrs: Cidrs,
    #[arg(
        long,
        env = "VDA_TRUSTED_PROXIES",
        alias = "trusted-proxies",
        default_value = ""
    )]
    pub trusted_proxy_cidrs: Cidrs,
    /// Backward-compatible deployment variable; merged with VDA_TRUSTED_PROXIES.
    #[arg(long, env = "VDA_TRUSTED_PROXY_CIDRS", default_value = "")]
    pub legacy_trusted_proxy_cidrs: Cidrs,
    #[arg(long, env="VDA_TRUST_PROXY_HEADERS", default_value="false", action=clap::ArgAction::Set)]
    pub trust_proxy_headers: bool,
    #[arg(long, env="VDA_ALLOW_PRIVATE_TARGETS", default_value="false", action=clap::ArgAction::Set)]
    pub allow_private_targets: bool,
    #[arg(long, env="VDA_LOG_FORMAT", default_value="text", value_parser=["text","json"])]
    pub log_format: String,
    #[arg(long, env = "VDA_METRICS_TOKEN", hide_env_values = true)]
    pub metrics_token: Option<SecretString>,
}
impl Config {
    /// Validate options before starting services.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.database_url.is_some(), "VDA_DATABASE_URL is required");
        anyhow::ensure!(self.master_key.is_some(), "VDA_MASTER_KEY is required");
        use secrecy::ExposeSecret;
        anyhow::ensure!(
            self.metrics_token
                .as_ref()
                .is_none_or(|token| !token.expose_secret().is_empty()),
            "VDA_METRICS_TOKEN must not be empty when set"
        );
        anyhow::ensure!(
            (1..=200).contains(&self.metadata_pool_size),
            "metadata pool size must be 1..=200"
        );
        anyhow::ensure!(
            self.bootstrap_admin_email.is_some() == self.bootstrap_admin_password.is_some(),
            "both bootstrap admin options must be supplied together"
        );
        Ok(())
    }
}

/// A comma-separated list of CIDRs; an explicitly empty environment value allows all.
#[derive(Debug, Clone, Default)]
pub struct Cidrs(pub Vec<IpNet>);
impl std::str::FromStr for Cidrs {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > 8192 {
            return Err("CIDR list is too large".into());
        }
        let networks = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().map_err(|_| "Invalid CIDR".to_owned()))
            .collect::<Result<Vec<IpNet>, String>>()?;
        if networks.len() > 128 {
            return Err("Too many CIDRs".into());
        }
        Ok(Self(networks))
    }
}
impl std::ops::Deref for Cidrs {
    type Target = [IpNet];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
