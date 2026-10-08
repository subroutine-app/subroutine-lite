use axum::{Json, Router, routing::get};
use subroutine_core::{AccountInfo, AccountProfile, UpdateAccountProfile};

use crate::{auth::Tenant, db, error::AppError, state::AppState};

const MAX_DISPLAY_NAME_SCALARS: usize = 80;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/account", get(account))
        .route("/account/profile", get(profile).put(replace_profile))
}

async fn account(Tenant(state): Tenant) -> crate::error::Result<Json<AccountInfo>> {
    let dataset_id = state.scope().dataset_id().await?;
    Ok(Json(AccountInfo {
        account_id: state.scope().user_id,
        dataset_id,
    }))
}

async fn profile(Tenant(state): Tenant) -> crate::error::Result<Json<AccountProfile>> {
    Ok(Json(db::account::profile(state.scope()).await?))
}

async fn replace_profile(
    Tenant(state): Tenant,
    Json(request): Json<UpdateAccountProfile>,
) -> crate::error::Result<Json<AccountProfile>> {
    let display_name = normalize_display_name(request.display_name)?;
    Ok(Json(
        db::account::replace_display_name(state.scope(), display_name.as_deref()).await?,
    ))
}

fn normalize_display_name(display_name: Option<String>) -> crate::error::Result<Option<String>> {
    let Some(display_name) = display_name else {
        return Ok(None);
    };
    let display_name = display_name.trim();
    let scalar_count = display_name.chars().count();
    if scalar_count == 0 {
        return Err(AppError::bad_request(
            "display name must not be blank".into(),
        ));
    }
    if scalar_count > MAX_DISPLAY_NAME_SCALARS {
        return Err(AppError::bad_request(format!(
            "display name must contain at most {MAX_DISPLAY_NAME_SCALARS} characters"
        )));
    }
    Ok(Some(display_name.to_owned()))
}
