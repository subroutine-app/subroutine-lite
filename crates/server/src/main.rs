use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod config;
mod db;

mod error;


pub(crate) use subroutine_core::ops;
mod routes;
mod state;

use state::AppState;

#[derive(Parser)]
#[command(name = "subroutine-lite-server", about = "Subroutine Lite server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Serve,
    Migrate {
        #[arg(long)]
        confirm: bool,
    },

    RotateDataset {
        #[arg(long)]
        user_id: uuid::Uuid,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        confirm: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "subroutine_lite_server=debug,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let command = Cli::parse().command.unwrap_or(Command::Serve);
    if matches!(
        &command,
        Command::RotateDataset { confirm: false, .. } | Command::Migrate { confirm: false }
    ) {
        bail!("refusing to run migrations or rotate a dataset without --confirm");
    }
    let serving = if matches!(&command, Command::Serve) {
        let verifier = auth::Verifier::from_env().await?;
        let listen = config::ListenConfig::from_env()?;
        Some((listen.bind_addr, verifier))
    } else {
        None
    };
    let pool = db::connect_from_env().await?;
    if matches!(&command, Command::Migrate { .. }) {
        return db::migrate(&pool).await;
    }
    db::ensure_schema(&pool).await?;

    match command {
        Command::Migrate { .. } => unreachable!("handled before schema validation"),

        Command::RotateDataset {
            user_id,
            actor,
            reason,
            confirm,
        } => {
            debug_assert!(confirm, "checked before database connection");
            let outcome = db::rotate_dataset(&pool, user_id, &actor, &reason).await?;
            tracing::info!(
                previous_dataset_id = %outcome.previous_dataset_id,
                dataset_id = %outcome.dataset_id,
                change_seq = outcome.change_seq,
                deleted_receipts = outcome.deleted_receipts,
                %user_id,
                %actor,
                "dataset rotation complete"
            );
            return Ok(());
        }
        Command::Serve => {}
    }

    let (bind_addr, verifier) =
        serving.expect("serving configuration was validated before database connection");
    let settings = config::settings_from_env();
    tracing::info!(?settings, "scheduling settings");

    let state = AppState::new(pool, settings).with_auth(verifier);
    spawn_recurrence_worker(state.clone());
    let app = application_router(state);
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("listening on {bind_addr}");
    axum::serve(listener, app).await?;

    Ok(())
}

fn application_router(state: AppState) -> axum::Router {
    let protected = routes::router().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth::authenticate_request,
    ));
    axum::Router::new().nest("/v1", protected).with_state(state)
}

fn spawn_recurrence_worker(state: AppState) {
    tokio::spawn(async move {
        loop {
            let worker = tokio::spawn(recurrence_worker_loop(state.clone()));
            match worker.await {
                Ok(()) => tracing::error!("recurrence worker exited unexpectedly"),
                Err(error) if error.is_panic() => {
                    tracing::error!(?error, "recurrence worker panicked; restarting")
                }
                Err(error) => tracing::error!(?error, "recurrence worker stopped; restarting"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

async fn recurrence_worker_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match state.reconcile_recurrence().await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "reconciled tenant lifecycle items"),
            Err(error) => tracing::error!(?error, "failed to enumerate tenant lifecycle work"),
        }
    }
}
