use axum::{
    Json,
    extract::{FromRequestParts, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
        request::Parts,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use subroutine_core::{ApiErrorBody, ApiErrorCode};

mod config;
mod verifier;
pub(crate) use verifier::Verifier;

use crate::state::{AppState, TenantState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthenticatedIdentity {
    pub(crate) issuer: String,
    pub(crate) subject: String,
}

pub(crate) struct Tenant(pub(crate) TenantState);

#[derive(Clone, Copy)]
pub(crate) struct TokenDeadline(pub(crate) tokio::time::Instant);

impl FromRequestParts<AppState> for TokenDeadline {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .copied()
            .ok_or(AuthError::MissingTenantContext)
    }
}

impl FromRequestParts<AppState> for Tenant {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TenantState>()
            .cloned()
            .map(Self)
            .ok_or(AuthError::MissingTenantContext)
    }
}

pub(crate) async fn authenticate_request(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let verified = match authenticate(request.headers(), &state).await {
        Ok(verified) => verified,
        Err(error) => return error.into_response(),
    };
    let tenant = match state.tenant_state(&verified.identity).await {
        Ok(tenant) => tenant,
        Err(error) => {
            tracing::error!(?error, "failed to resolve authenticated user");
            return AuthError::IdentityResolution.into_response();
        }
    };
    if verified.deadline.0 <= tokio::time::Instant::now() {
        return AuthError::InvalidToken.into_response();
    }
    request.extensions_mut().insert(verified.deadline);
    request.extensions_mut().insert(tenant);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
}

async fn authenticate(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<verifier::VerifiedToken, AuthError> {
    let token = bearer_token(headers)?;
    state
        .auth_verifier()
        .ok_or(AuthError::Unavailable)?
        .verify(token)
        .await
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let value = headers
        .get(AUTHORIZATION)
        .ok_or(AuthError::MissingToken)?
        .to_str()
        .map_err(|_| AuthError::InvalidToken)?;
    if headers.get_all(AUTHORIZATION).iter().count() != 1 {
        return Err(AuthError::InvalidToken);
    }
    let (scheme, token) = value.split_once(' ').ok_or(AuthError::InvalidToken)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
    {
        return Err(AuthError::InvalidToken);
    }
    Ok(token)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthError {
    MissingToken,
    InvalidToken,
    InsufficientScope,
    Unavailable,
    MissingTenantContext,
    IdentityResolution,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code, message, retryable) = match self {
            Self::MissingToken => (
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::MissingToken,
                "A bearer access token is required.",
                false,
            ),
            Self::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::InvalidToken,
                "The bearer access token is invalid or expired.",
                false,
            ),
            Self::InsufficientScope => (
                StatusCode::FORBIDDEN,
                ApiErrorCode::InsufficientScope,
                "The subroutine:sync scope is required.",
                false,
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::AuthenticationUnavailable,
                "Authentication is unavailable.",
                true,
            ),
            Self::MissingTenantContext | Self::IdentityResolution => (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::AuthenticationUnavailable,
                "Authentication is temporarily unavailable.",
                true,
            ),
        };
        let body = ApiErrorBody {
            error: code,
            message: message.into(),
            mutation_id: None,
            resource: None,
            current_seq: None,
            current_dataset_id: None,
            retryable,
        };
        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        } else if status == StatusCode::FORBIDDEN {
            response.headers_mut().insert(
                WWW_AUTHENTICATE,
                HeaderValue::from_static(
                    "Bearer error=\"insufficient_scope\", scope=\"subroutine:sync\"",
                ),
            );
        }
        response
    }
}
