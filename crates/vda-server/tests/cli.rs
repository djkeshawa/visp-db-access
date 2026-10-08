#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use base64::Engine;

#[test]
fn key_generation_works_without_database_or_existing_key() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_visp-db-access"))
        .arg("gen-key")
        .env_remove("VDA_DATABASE_URL")
        .env_remove("VDA_MASTER_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    let encoded = std::str::from_utf8(&output.stdout).unwrap().trim();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap()
            .len(),
        32
    );
}

#[test]
fn serving_refuses_missing_master_key() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_visp-db-access"))
        .arg("serve")
        .env("VDA_DATABASE_URL", "postgres://unused@127.0.0.1:9/unused")
        .env_remove("VDA_MASTER_KEY")
        .env_remove("VDA_TRUST_PROXY_HEADERS")
        .env_remove("VDA_BOOTSTRAP_ADMIN_EMAIL")
        .env_remove("VDA_BOOTSTRAP_ADMIN_PASSWORD")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("VDA_MASTER_KEY is required"));
}

#[test]
fn explicitly_empty_cidr_lists_are_valid_configuration() {
    use clap::Parser;
    let cli = vda_server::config::Cli::try_parse_from([
        "test",
        "--allowed-cidrs",
        "",
        "--trusted-proxy-cidrs",
        "",
    ]);
    assert!(cli.is_ok(), "{cli:?}");
    let cli = cli.unwrap();
    assert!(cli.config.allowed_cidrs.is_empty());
    assert!(cli.config.trusted_proxy_cidrs.is_empty());
}
