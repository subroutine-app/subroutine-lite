pub(crate) mod account;
pub(crate) mod actions;
mod convert;
pub(crate) mod events;
mod markers;
pub(crate) mod mutations;

mod record;
mod signals;
mod store;
mod tenant;

pub(crate) mod routines;

pub(crate) use record::{Record, fetch_all, fetch_by_id, fetch_by_id_in, identity_exists_in};
pub(crate) use store::{ApplyResult, all_data, apply_changes_in, data_delta_in, snapshot_in};
pub(crate) use tenant::{Sequenced, TenantMutation, TenantScope, rotate_dataset};

pub(crate) use sqlx::PgPool;

use anyhow::Context as _;

fn connect_options(url: &str) -> anyhow::Result<sqlx::postgres::PgConnectOptions> {
    let parsed = url::Url::parse(url)
        .map_err(|_| anyhow::anyhow!("SUBROUTINE_LITE_DATABASE_URL must be a PostgreSQL URL"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "postgres" | "postgresql"),
        "SUBROUTINE_LITE_DATABASE_URL must be a PostgreSQL URL"
    );
    let explicit_database = parsed
        .query_pairs()
        .filter(|(key, _)| key == "dbname")
        .last()
        .map(|(_, value)| !value.trim().is_empty())
        .unwrap_or_else(|| !parsed.path().trim_start_matches('/').trim().is_empty());
    anyhow::ensure!(
        explicit_database,
        "SUBROUTINE_LITE_DATABASE_URL must explicitly name a distinct Lite database"
    );
    let options: sqlx::postgres::PgConnectOptions = url.parse().map_err(|_| {
        anyhow::anyhow!(
            "SUBROUTINE_LITE_DATABASE_URL contains invalid PostgreSQL connection options"
        )
    })?;
    validate_database_name(options.get_database().unwrap_or_default())?;
    Ok(options)
}

fn validate_database_name(name: &str) -> anyhow::Result<()> {
    let name = name.to_ascii_lowercase();
    let lite = ["subroutine_lite", "subroutine-lite"].iter().any(|prefix| {
        name == *prefix
            || name
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with(['_', '-']))
    });
    let full =
        name == "subroutine" || name.starts_with("subroutine_") || name.starts_with("subroutine-");
    anyhow::ensure!(
        !name.trim().is_empty(),
        "SUBROUTINE_LITE_DATABASE_URL must explicitly name a distinct Lite database"
    );
    anyhow::ensure!(
        !full || lite,
        "SUBROUTINE_LITE_DATABASE_URL targets a reserved full-app database (subroutine or subroutine_* / subroutine-* outside the Lite namespace); use subroutine_lite or a distinct custom Lite/test database"
    );
    Ok(())
}

pub(crate) async fn connect_from_env() -> anyhow::Result<PgPool> {
    let url = std::env::var("SUBROUTINE_LITE_DATABASE_URL").map_err(|_| {
        anyhow::anyhow!("SUBROUTINE_LITE_DATABASE_URL must be set to a PostgreSQL URL")
    })?;
    connect(&url).await
}

pub(crate) async fn connect(url: &str) -> anyhow::Result<PgPool> {
    let options = connect_options(url)?;
    tracing::info!("connecting to database");
    let pool = PgPool::connect_with(options)
        .await
        .context("connect to postgres")?;
    tracing::info!("connected");
    Ok(pool)
}

pub(crate) async fn ensure_schema(pool: &PgPool) -> anyhow::Result<()> {
    let mut connection = pool
        .acquire()
        .await
        .context("acquire schema-check connection")?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&mut *connection)
        .await
        .context("verify schema-check database")?;
    validate_database_name(&database)?;
    let applied: Vec<(i64, bool, Vec<u8>)> = sqlx::query_as(
        "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version"
    ).fetch_all(&mut *connection).await.context(
        "Cannot verify Lite schema. After explicit approval, run subroutine-lite-server migrate --confirm against an isolated Lite database"
    )?;
    let migrator = sqlx::migrate!();
    let expected: Vec<_> = migrator
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .collect();
    anyhow::ensure!(
        applied.len() == expected.len()
            && applied
                .iter()
                .zip(expected)
                .all(|((version, success, checksum), migration)| {
                    *version == migration.version
                        && *success
                        && checksum.as_slice() == migration.checksum.as_ref()
                }),
        "Lite database schema differs from this binary; startup never runs migrations. Review the database and explicitly approve migration before retrying"
    );
    Ok(())
}

pub(crate) async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    let mut connection = pool
        .acquire()
        .await
        .context("acquire migration connection")?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&mut *connection)
        .await
        .context("verify migration database")?;
    validate_database_name(&database)?;
    tracing::info!("running migrations");
    sqlx::migrate!()
        .run(&mut *connection)
        .await
        .context("run migrations")?;
    tracing::info!("migrations complete");
    Ok(())
}
