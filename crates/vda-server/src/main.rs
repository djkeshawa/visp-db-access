//! CLI lifecycle and graceful gateway shutdown.
use clap::Parser;
use secrecy::ExposeSecret;
use std::io::Write;
use vda_server::{
    app::{router, AppState},
    config::{Cli, Command},
    crypto::Crypto,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::GenKey)) {
        use base64::Engine;
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        writeln!(
            std::io::stdout(),
            "{}",
            base64::engine::general_purpose::STANDARD.encode(key)
        )?;
        return Ok(());
    }
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    if cli.config.log_format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
    cli.config.validate()?;
    let master = cli
        .config
        .master_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("VDA_MASTER_KEY is required"))?;
    let crypto = Crypto::from_base64(master.expose_secret())?;
    let url = cli
        .config
        .database_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("VDA_DATABASE_URL is required"))?;
    let db = vda_server::db::connect(url.expose_secret(), cli.config.metadata_pool_size).await?;
    vda_server::db::migrate(&db).await?;
    match cli.command.unwrap_or(Command::Serve) {
        Command::Migrate => {
            tracing::info!("metadata migrations applied");
            db.close().await;
            Ok(())
        }
        Command::CreateAdmin {
            email,
            name,
            password,
        } => {
            anyhow::ensure!(
                email.contains('@')
                    && email.len() <= 254
                    && !email.chars().any(char::is_whitespace),
                "invalid email"
            );
            anyhow::ensure!(!name.trim().is_empty() && name.len() <= 256, "invalid name");
            vda_server::sanitation::text(&email)?;
            vda_server::sanitation::text(&name)?;
            let hash = vda_server::auth::hash_password(password.expose_secret().to_owned()).await?;
            let mut tx = db.begin().await?;
            let id = uuid::Uuid::new_v4();
            sqlx::query(
"INSERT INTO users(id,email,name,password_hash,org_role) VALUES ($1,$2,$3,$4,'admin')",
)
.bind(id)
.bind(email.trim().to_lowercase())
.bind(name)
.bind(hash)
.execute(&mut *tx).await?;
            vda_server::audit::persist_on(
                &mut tx,
                &vda_server::audit::Event {
                    id: uuid::Uuid::new_v4(),
                    actor_id: None,
                    action: "user.create_admin".into(),
                    target_type: Some("user".into()),
                    target_id: Some(id),
                    ip: None,
                    details: serde_json::json!({}),
                    created_at: chrono::Utc::now(),
                },
            )
            .await?;
            tx.commit().await?;
            tracing::info!("administrator created");
            db.close().await;
            Ok(())
        }
        Command::Serve => serve(db, cli.config, crypto).await,
        Command::GenKey => Ok(()),
    }
}
async fn serve(
    db: sqlx::PgPool,
    config: vda_server::config::Config,
    crypto: Crypto,
) -> anyhow::Result<()> {
    if !config.cookie_secure {
        tracing::warn!("VDA_COOKIE_SECURE=false: session cookies may be sent over plaintext HTTP");
    }
    let prometheus = vda_server::metrics::install()?;
    let bind = config.bind;
    let (state, writer) = AppState::new(db, config, crypto, Some(prometheus)).await?;
    vda_server::auth::bootstrap(&state).await?;
    let discovery_scheduler = tokio::spawn(vda_server::discovery::scheduler(state.clone()));
    let scheduler = tokio::spawn(vda_server::health::scheduler(state.clone()));
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind,"gateway listening");
    let shutdown_state = state.clone();
    let result = axum::serve(
        listener,
        router(state.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        for query in shutdown_state.active.iter() {
            query.1.cancel();
        }
    })
    .await;
    state.tasks.close();
    state.tasks.wait().await;
    state.stop.cancel();
    scheduler.await?;
    discovery_scheduler.await?;
    // All HTTP handlers have drained; close the sender side through receiver.close.
    // Let bounded queued events finish before closing metadata storage.
    writer.await?;
    state.pools.close().await;
    state.db.close().await;
    result?;
    Ok(())
}
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error,"cannot install SIGINT handler");
        }
    };
    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => {
                    tracing::error!(%error,"cannot install SIGTERM handler");
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! {_=ctrl_c=>{},_=terminate=>{}}
    }
    #[cfg(not(unix))]
    ctrl_c.await;
}
