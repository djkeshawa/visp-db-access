//! TLS and connection option construction without credential-bearing URLs.
use crate::{ConnectionSpec, TlsMode};
use secrecy::ExposeSecret;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlSslMode},
    postgres::{PgConnectOptions, PgSslMode},
};

fn pg_mode(mode: TlsMode) -> PgSslMode {
    match mode {
        TlsMode::Disable => PgSslMode::Disable,
        TlsMode::Prefer => PgSslMode::Prefer,
        TlsMode::Require => PgSslMode::Require,
        TlsMode::VerifyFull => PgSslMode::VerifyFull,
    }
}
fn mysql_mode(mode: TlsMode) -> MySqlSslMode {
    match mode {
        TlsMode::Disable => MySqlSslMode::Disabled,
        TlsMode::Prefer => MySqlSslMode::Preferred,
        TlsMode::Require => MySqlSslMode::Required,
        TlsMode::VerifyFull => MySqlSslMode::VerifyIdentity,
    }
}
pub(crate) fn postgres(spec: &ConnectionSpec) -> PgConnectOptions {
    let mut options = PgConnectOptions::new()
        .host(&spec.host)
        .port(spec.port)
        .database(&spec.database)
        .username(&spec.username)
        .password(spec.password.expose_secret())
        .application_name(&spec.application_name)
        .ssl_mode(pg_mode(spec.tls))
        .statement_cache_capacity(0);
    if spec.tls == TlsMode::VerifyFull {
        if let Some(pem) = &spec.ca_cert_pem {
            options = options.ssl_root_cert_from_pem(pem.as_bytes().to_vec());
        }
    }
    options
}
pub(crate) fn mysql(spec: &ConnectionSpec) -> MySqlConnectOptions {
    let mut options = MySqlConnectOptions::new()
        .host(&spec.host)
        .port(spec.port)
        .database(&spec.database)
        .username(&spec.username)
        .password(spec.password.expose_secret())
        .ssl_mode(mysql_mode(spec.tls))
        .statement_cache_capacity(0);
    if spec.tls == TlsMode::VerifyFull {
        if let Some(pem) = &spec.ca_cert_pem {
            options = options.ssl_ca_from_pem(pem.as_bytes().to_vec());
        }
    }
    options
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_preserve_verification_and_encryption() {
        for (input, pg, mysql) in [
            (TlsMode::Disable, PgSslMode::Disable, MySqlSslMode::Disabled),
            (TlsMode::Prefer, PgSslMode::Prefer, MySqlSslMode::Preferred),
            (TlsMode::Require, PgSslMode::Require, MySqlSslMode::Required),
            (
                TlsMode::VerifyFull,
                PgSslMode::VerifyFull,
                MySqlSslMode::VerifyIdentity,
            ),
        ] {
            assert_eq!(
                std::mem::discriminant(&pg_mode(input)),
                std::mem::discriminant(&pg)
            );
            assert_eq!(
                std::mem::discriminant(&mysql_mode(input)),
                std::mem::discriminant(&mysql)
            );
        }
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;
    use sqlx::ConnectOptions;
    #[test]
    fn options_preserve_fields_without_url_interpolation_or_statement_caching() {
        let spec = ConnectionSpec {
            engine: crate::Engine::Postgres,
            host: "db.example".into(),
            port: 5433,
            database: "target_db".into(),
            username: "reader".into(),
            password: secrecy::SecretString::from("p@ss:/?#"),
            tls: TlsMode::VerifyFull,
            ca_cert_pem: Some("fixture PEM".into()),
            application_name: "vda:'quoted'".into(),
        };
        let pg = postgres(&spec);
        let mysql = mysql(&spec);
        assert_eq!(pg.get_host(), spec.host);
        assert_eq!(pg.get_port(), spec.port);
        assert_eq!(pg.get_database(), Some(spec.database.as_str()));
        assert_eq!(pg.get_username(), spec.username);
        assert_eq!(
            pg.get_application_name(),
            Some(spec.application_name.as_str())
        );
        assert!(matches!(pg.get_ssl_mode(), PgSslMode::VerifyFull));
        assert_eq!(mysql.get_host(), spec.host);
        assert_eq!(mysql.get_port(), spec.port);
        assert_eq!(mysql.get_database(), Some(spec.database.as_str()));
        assert_eq!(mysql.get_username(), spec.username);
        assert!(matches!(mysql.get_ssl_mode(), MySqlSslMode::VerifyIdentity));
        assert!(pg
            .to_url_lossy()
            .query_pairs()
            .any(|(name, value)| name == "statement-cache-capacity" && value == "0"));
        assert!(mysql
            .to_url_lossy()
            .query_pairs()
            .any(|(name, value)| name == "statement-cache-capacity" && value == "0"));
    }
}
