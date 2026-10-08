use std::time::Duration;

use reqwest::{Client, RequestBuilder, StatusCode, header::HeaderMap};
use serde::de::DeserializeOwned;
use url::Url;

use crate::ProviderError;

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;

pub(crate) fn client() -> Result<Client, ProviderError> {
    Client::builder()
        .use_rustls_tls()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| ProviderError::HttpClient)
}

pub(crate) async fn send(
    request: RequestBuilder,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), ProviderError> {
    let mut response = request.send().await.map_err(|_| ProviderError::Request)?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_DOCUMENT_BYTES as u64)
    {
        return Err(ProviderError::DocumentTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ProviderError::Request)? {
        if chunk.len() > MAX_DOCUMENT_BYTES - bytes.len() {
            return Err(ProviderError::DocumentTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((response.status(), response.headers().clone(), bytes))
}

pub(crate) async fn document<T: DeserializeOwned>(
    http: &Client,
    url: &Url,
) -> Result<T, ProviderError> {
    let (status, _, bytes) = send(http.get(url.clone())).await?;
    if !status.is_success() {
        return Err(ProviderError::HttpStatus(status.as_u16()));
    }
    serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidDocument)
}
