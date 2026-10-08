use anyhow::Context as _;
use chrono::{DateTime, Utc};
use subroutine_core::AccountProfile;

use super::TenantScope;

#[derive(sqlx::FromRow)]
struct ProfileRow {
    display_name: Option<String>,
    updated_at: DateTime<Utc>,
}

pub(crate) async fn profile(scope: &TenantScope) -> anyhow::Result<AccountProfile> {
    let row: ProfileRow =
        sqlx::query_as("SELECT display_name, updated_at FROM account_profiles WHERE user_id = $1")
            .bind(scope.user_id)
            .fetch_one(&scope.pool)
            .await
            .context("read account profile")?;
    Ok(AccountProfile {
        display_name: row.display_name,
        updated_at: row.updated_at,
    })
}

pub(crate) async fn replace_display_name(
    scope: &TenantScope,
    display_name: Option<&str>,
) -> anyhow::Result<AccountProfile> {
    let mut mutation = scope.begin_mutation().await?;
    let row: ProfileRow = sqlx::query_as(
        "UPDATE account_profiles \
         SET display_name = $2, updated_at = NOW() \
         WHERE user_id = $1 \
         RETURNING display_name, updated_at",
    )
    .bind(scope.user_id)
    .bind(display_name)
    .fetch_one(mutation.connection())
    .await
    .context("replace account display name")?;
    mutation.commit().await?;
    Ok(AccountProfile {
        display_name: row.display_name,
        updated_at: row.updated_at,
    })
}
