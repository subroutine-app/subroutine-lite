use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use openidconnect::{
    JsonWebKeyId, PrivateSigningKey,
    core::{CoreJsonWebKeySet, CoreRsaPrivateSigningKey},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};

use super::{Config, FailureKind, Provider, TokenError, Tokens};
use crate::{Issuer, TransportSecurity};

// Generated exclusively for these tests; never a deployment credential.
const KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAliGg1dce0ZN7J6CekDtwepwyCyAzqqgpsCcuf2/uU/RHm1Cn
RxilWvMmUfhis96G8SwiD6kVQtp9s2VtzgaXB7nU77oLf1qnVHIq/NTko0Keheh2
G+R/GbSXXeQVYPh/Sw1p4qbsnKcu8GCennQ37hR1sSNgxQWl9EXgf49SWChKdbP3
Z2g6fKM+TgY4sfnvpMlCjUso5OA89/M8HM3FGOJM6TW8llcbTTl0IUTQbcAVsERG
LCSo5GImXO9hZC5y3KUv4Gi5CFZlZAS7qLc5ue/7zulbGGKENR7Euf7pXsS2U9OY
tCJ4Dkafc14EaryakmfQ9aaSoJUfRCzu3t2TMQIDAQABAoIBACOelOdHY8N7h0Ou
oxA7WgNtHZ5wJHH/9ewaT7jimJwIoBikeRuwKEM830GheeGNEV0VCHvItQeoiRxQ
G04/WAP4PiImdOXhX0P01mgoJprpimzg62zQuKsjoqofHEdmjeUDqLSiwA6mx8c2
VUvo7QAqkIXXi/US0v3yukHpgjLx9ym/bGgDmVJIq3mOi1TI2v7FuYoZ8PtV2COO
rzxzVgoGMfFRVwUPf0kq7Ug+1hHb1sqXgpOhaauzrSoOzwuKY1xc7pHjg/D2THNM
JDVw2H57OT4okZUttT333LKSlLtXBmOQBA8aKDhyLYdU0KDBgXm2k8MNDeGBpy67
sbhBpSUCgYEAy1vg81MbtwK14lPGxZ4l9Ffn6fAQh7dzu7RFIO2oZ6SK16+HEhKJ
hFQcveaI/kn8N0INg/mGFyAiJ0vu/lEGtSniwB6gpbrxuc/rNPBxlQ/ITmGqjo3C
STc5+KG4VQw31taudW3GMysoxGw9q47aD0NjCw+NCd41ix8h64sy20sCgYEAvP57
98HH488+g8R50QhVQuA0mZFd8ZVdTQAyXZTicmx8AjtS9KEqn2ATNGQOe1iZj9tZ
bd3L8H/A8ftaCEDRyJ45V1wyXsx5vMpF9Fg0BxkYOOM/GX+wX/FH0Vb3YdQTxIcI
h4oGNL4Q3adZYR1PnWphjolHPVg0RE3TmpmAYfMCgYEAo+320m9mYgr/T+k0zM0F
z4ilXMG2sZDC7McPCtJJ9PKBMgk/GuRykoeze6nBOSuu82R9vKuC2lV+8qqVh9EN
VTDkpvaJjc+7WIwxEMXa6cyFLhoxASVpPtgUFUdYbsagpMD+dY7+PPVHsb+HnypS
wgcSajIpAZFtHC8Givf1ES0CgYEAuQL3IIcWwzByJlQEXFkn/hu14k+oA4MQnvx+
UZYaR6FjlFu2zbuoGfTXWAZLFvyY93bniOIDbx/hJgHFFSc/NoGN6LNcq+jHk8T/
zj86T2iFicsMTk3tvz5X69qSubX+0/jMpMK9aIxXvmTA3ozvD07AyeQhVTAvml4F
s+ZpA3kCgYB5UMglUdGNdkrwHnunlfFApevEjZJ2E80Xn/Qmtn9tDxtbZC7MeJSn
U9KIpopvI/YkHTUqSIZGPUNcuHmdg+WQIJHRH2MGCOXTSXiH0tnxKjYLnxgv+ZcR
GXDpVJK5gf7Hbp0JJzq2WHr/o1MmVFURciRNsY1v2+axM4s23HSSuQ==
-----END RSA PRIVATE KEY-----";

pub(super) fn keys() -> CoreJsonWebKeySet {
    let key =
        CoreRsaPrivateSigningKey::from_pem(KEY, Some(JsonWebKeyId::new("test".into()))).unwrap();
    CoreJsonWebKeySet::new(vec![key.as_verification_key()])
}

pub(super) fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test".into());
    jsonwebtoken::encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(KEY.as_bytes()).unwrap(),
    )
    .unwrap()
}

pub(super) fn claims(issuer: &str) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({
        "iss": issuer, "aud": "subroutine-desktop", "azp": "subroutine-desktop",
        "sub": "test-subject", "iat": now, "exp": now + 300,
        "nonce": "test-nonce", "custom": {"display_name": "Fixture ✨"}
    })
}

pub(super) fn config(issuer: &str) -> Config {
    Config::new(
        Issuer::with_security(issuer, TransportSecurity::InsecureLoopback).unwrap(),
        "subroutine-desktop",
        "http://127.0.0.1:43119/oidc/callback",
        vec!["openid".into(), "subroutine:sync".into()],
    )
    .unwrap()
}

fn response(id_token: Value, rotation: &str) -> Value {
    json!({
        "access_token": format!("access-{rotation}"), "refresh_token": format!("refresh-{rotation}"),
        "token_type": "Bearer", "expires_in": 300, "scope": "openid subroutine:sync",
        "id_token": id_token,
    })
}

fn signed_tokens(issuer: &str) -> Tokens {
    let now = chrono::Utc::now().timestamp();
    Tokens::from_response(
        serde_json::from_value(response(json!(sign(&claims(issuer))), "new")).unwrap(),
        config(issuer).required_scopes(),
        now,
        now,
        None,
        super::id_token::NonceContext::Code("test-nonce".into()),
    )
    .unwrap()
}

fn ready(tokens: &Tokens) -> Tokens {
    let mut stored = serde_json::to_value(tokens).unwrap();
    let now = chrono::Utc::now().timestamp();
    stored["expires_at"] = json!(now - 1);
    stored["refresh_not_before"] = json!(now - 1);
    serde_json::from_value(stored).unwrap()
}

struct Reply {
    body: Value,
    status: &'static str,
    delay: Duration,
}

struct Server {
    issuer: String,
    replies: Arc<Mutex<HashMap<String, VecDeque<Reply>>>>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<Vec<String>>,
}

impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let replies = Arc::new(Mutex::new(HashMap::<String, VecDeque<Reply>>::new()));
        let routes = replies.clone();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut workers = JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! {
                    accepted = listener.accept() => accepted.unwrap(),
                    _ = &mut stopped => break,
                };
                let routes = routes.clone();
                workers.spawn(async move {
                    let mut bytes = Vec::new();
                    let request = loop {
                        let mut buffer = [0; 4096];
                        let count = stream.read(&mut buffer).await.unwrap();
                        assert_ne!(count, 0);
                        bytes.extend_from_slice(&buffer[..count]);
                        let text = std::str::from_utf8(&bytes).unwrap();
                        if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                            let length = headers.lines().find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            }).unwrap_or(0);
                            if body.len() >= length { break text.to_owned(); }
                        }
                    };
                    let target = request.lines().next().unwrap();
                    let reply = routes.lock().unwrap().get_mut(target)
                        .and_then(VecDeque::pop_front).expect("unexpected request");
                    tokio::time::sleep(reply.delay).await;
                    let body = reply.body.to_string();
                    let written = stream.write_all(format!(
                        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", reply.status, body.len()
                    ).as_bytes()).await;
                    // Budgeted requests may disconnect before a delayed response.
                    if reply.delay.is_zero() { written.unwrap(); }
                    request
                });
            }
            let mut requests = Vec::new();
            while let Some(request) = workers.join_next().await {
                requests.push(request.unwrap());
            }
            requests
        });
        Self {
            issuer,
            replies,
            stop,
            task,
        }
    }

    fn reply(&self, target: &str, body: Value) {
        self.delayed_reply(target, body, "200 OK", Duration::ZERO);
    }

    fn delayed_reply(&self, target: &str, body: Value, status: &'static str, delay: Duration) {
        self.replies
            .lock()
            .unwrap()
            .entry(target.into())
            .or_default()
            .push_back(Reply {
                body,
                status,
                delay,
            });
    }

    fn discovery(&self, identity: bool, delay: Duration) {
        let mut metadata = json!({
            "issuer": self.issuer, "authorization_endpoint": format!("{}/authorize", self.issuer),
            "token_endpoint": format!("{}/token", self.issuer),
            "userinfo_endpoint": format!("{}/userinfo", self.issuer),
            "response_types_supported": ["code"], "code_challenge_methods_supported": ["S256"],
            "authorization_response_iss_parameter_supported": true,
        });
        if identity {
            metadata["jwks_uri"] = json!(format!("{}/jwks", self.issuer));
            metadata["id_token_signing_alg_values_supported"] = json!(["RS256"]);
        }
        self.delayed_reply(
            "GET /.well-known/openid-configuration HTTP/1.1",
            metadata,
            "200 OK",
            delay,
        );
    }

    async fn provider(&self, identity: bool) -> Provider {
        self.discovery(identity, Duration::ZERO);
        Provider::discover(config(&self.issuer)).await.unwrap()
    }

    fn profile(&self, jwks: bool) {
        self.reply(
            "GET /userinfo HTTP/1.1",
            json!({"sub": "test-subject", "name": "Provider name"}),
        );
        if jwks {
            self.reply("GET /jwks HTTP/1.1", serde_json::to_value(keys()).unwrap());
        }
    }

    async fn finish(self) -> Vec<String> {
        self.stop.send(()).unwrap();
        let requests = tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            self.replies
                .lock()
                .unwrap()
                .values()
                .all(VecDeque::is_empty)
        );
        requests
    }
}

#[tokio::test]
async fn code_and_refresh_replace_the_snapshot_without_changing_scopes_or_account_authority() {
    let server = Server::new().await;
    let provider = server.provider(true).await;
    let mut pending = provider.authorize();
    let params: HashMap<_, _> = pending
        .authorization_url()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(params["scope"], "openid subroutine:sync");
    assert_eq!(params["code_challenge_method"], "S256");
    assert!(!params["nonce"].is_empty());
    assert_ne!(
        params["nonce"],
        provider
            .authorize()
            .authorization_url()
            .query_pairs()
            .find(|(k, _)| k == "nonce")
            .unwrap()
            .1
    );
    let mut identity = claims(&server.issuer);
    identity["nonce"] = json!(params["nonce"]);
    server.reply(
        "POST /token HTTP/1.1",
        response(json!(sign(&identity)), "one"),
    );
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("state", &params["state"])
        .append_pair("code", "test-code")
        .append_pair("iss", &server.issuer)
        .finish();
    let code = pending.validate_callback(&query).unwrap();
    let tokens = pending.exchange(code).await.unwrap();
    let tokens: Tokens = serde_json::from_str(&serde_json::to_string(&tokens).unwrap()).unwrap();
    server.profile(true);
    let info = provider.userinfo(&tokens).await.unwrap();
    assert_eq!(info.subject(), "test-subject");
    assert_eq!(info.name(), Some("Fixture ✨"));
    assert!(matches!(
        provider.refresh(&tokens).await,
        Err(TokenError::NotReady { .. })
    ));

    identity.as_object_mut().unwrap().remove("nonce");
    identity.as_object_mut().unwrap().remove("custom");
    server.reply(
        "POST /token HTTP/1.1",
        response(json!(sign(&identity)), "two"),
    );
    let refreshed = provider.refresh(&ready(&tokens)).await.unwrap();
    server.profile(true);
    assert_eq!(
        provider.userinfo(&refreshed).await.unwrap().name(),
        Some("Provider name")
    );
    assert_eq!(refreshed.refresh_token(), "refresh-two");

    let mut absent = response(Value::Null, "three");
    absent.as_object_mut().unwrap().remove("id_token");
    server.reply("POST /token HTTP/1.1", absent);
    let refreshed = provider.refresh(&ready(&refreshed)).await.unwrap();
    server.profile(false);
    assert_eq!(
        provider.userinfo(&refreshed).await.unwrap().name(),
        Some("Provider name")
    );
    assert_eq!(
        refreshed.id_token_snapshot().unwrap().nonce(),
        Some(params["nonce"].as_str())
    );

    identity["custom"] = json!({"display_name": "Other identity"});
    identity["sub"] = json!("other-subject");
    server.reply(
        "POST /token HTTP/1.1",
        response(json!(sign(&identity)), "four"),
    );
    let refreshed = provider.refresh(&ready(&refreshed)).await.unwrap();
    server.profile(true);
    assert_eq!(
        provider.userinfo(&refreshed).await.unwrap().name(),
        Some("Provider name")
    );

    server.reply(
        "POST /token HTTP/1.1",
        response(json!({"malformed": true}), "five"),
    );
    let refreshed = provider.refresh(&ready(&refreshed)).await.unwrap();
    server.profile(false);
    assert_eq!(
        provider.userinfo(&refreshed).await.unwrap().name(),
        Some("Provider name")
    );
    assert_eq!(refreshed.refresh_token(), "refresh-five");

    let requests = server.finish().await;
    let token_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.starts_with("POST /token"))
        .collect();
    assert_eq!(token_requests.len(), 5);
    let body: HashMap<_, _> = url::form_urlencoded::parse(
        token_requests[0]
            .split_once("\r\n\r\n")
            .unwrap()
            .1
            .as_bytes(),
    )
    .into_owned()
    .collect();
    assert_eq!(body["grant_type"], "authorization_code");
    assert!(body.contains_key("code_verifier"));
    for request in &token_requests[1..] {
        let body: HashMap<_, _> =
            url::form_urlencoded::parse(request.split_once("\r\n\r\n").unwrap().1.as_bytes())
                .into_owned()
                .collect();
        assert_eq!(body["grant_type"], "refresh_token");
        assert!(!body.contains_key("scope"));
    }
    assert!(
        requests
            .iter()
            .filter(|r| r.starts_with("GET /jwks"))
            .all(|r| !r.to_ascii_lowercase().contains("authorization:"))
    );
}

#[tokio::test]
async fn missing_optional_discovery_metadata_and_legacy_credentials_use_userinfo() {
    let server = Server::new().await;
    let provider = server.provider(false).await;
    let stored =
        json!({"access": "old-access", "refresh": "old-refresh", "expires_at": 1700000000});
    let legacy: Tokens = serde_json::from_value(stored.clone()).unwrap();
    assert!(legacy.id_token_snapshot().is_none());
    assert_eq!(legacy.refresh_ready_at(), legacy.expires_at());
    server.profile(false);
    assert_eq!(
        provider.userinfo(&legacy).await.unwrap().name(),
        Some("Provider name")
    );
    server.reply(
        "POST /token HTTP/1.1",
        response(json!(sign(&claims(&server.issuer))), "new"),
    );
    let refreshed = provider.refresh(&legacy).await.unwrap();
    server.profile(false);
    assert_eq!(
        provider.userinfo(&refreshed).await.unwrap().name(),
        Some("Provider name")
    );
    server.finish().await;

    let mut malformed = stored;
    malformed["id_token"] = json!({"arbitrary_verified_name": "Untrusted"});
    let loaded: Tokens = serde_json::from_value(malformed).unwrap();
    assert!(loaded.id_token_snapshot().is_none());
    assert!(!format!("{loaded:?}").contains("old-access"));
}

#[tokio::test]
async fn slow_userinfo_and_unavailable_keys_still_return_fresh_fallback_within_profile_budget() {
    let server = Server::new().await;
    let provider = server.provider(true).await;
    let tokens = signed_tokens(&server.issuer);
    server.delayed_reply(
        "GET /userinfo HTTP/1.1",
        json!({"sub": "test-subject", "name": "Fresh provider name"}),
        "200 OK",
        Duration::from_millis(2500),
    );
    server.delayed_reply(
        "GET /jwks HTTP/1.1",
        Value::Null,
        "503 Service Unavailable",
        Duration::from_millis(1500),
    );
    let info = tokio::time::timeout(Duration::from_secs(3), provider.userinfo(&tokens))
        .await
        .expect("optional JWKS must not extend the UserInfo wait")
        .unwrap();
    assert_eq!(info.subject(), "test-subject");
    assert_eq!(info.name(), Some("Fresh provider name"));
    server.finish().await;
}

#[tokio::test]
async fn restore_budget_preserves_fresh_userinfo_when_discovery_leaves_little_time_for_keys() {
    let server = Server::new().await;
    let tokens = signed_tokens(&server.issuer);
    server.discovery(true, Duration::from_millis(2500));
    server.delayed_reply(
        "GET /userinfo HTTP/1.1",
        json!({"sub": "test-subject", "name": "Fresh provider name"}),
        "200 OK",
        Duration::from_millis(100),
    );
    server.delayed_reply(
        "GET /jwks HTTP/1.1",
        Value::Null,
        "503 Service Unavailable",
        Duration::from_millis(1500),
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let request = async {
        let provider =
            tokio::time::timeout_at(deadline, Provider::discover(config(&server.issuer)))
                .await
                .unwrap()
                .unwrap();
        provider
            .userinfo_with_timeout(
                &tokens,
                deadline.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await
            .unwrap()
    };
    let info = tokio::time::timeout(Duration::from_millis(3200), request)
        .await
        .expect("completed UserInfo must survive the shared deadline");
    assert_eq!(info.subject(), "test-subject");
    assert_eq!(info.name(), Some("Fresh provider name"));
    server.finish().await;
}

#[tokio::test]
async fn required_userinfo_outside_its_budget_errors_even_when_keys_are_ready() {
    let server = Server::new().await;
    let provider = server.provider(true).await;
    let tokens = signed_tokens(&server.issuer);
    server.delayed_reply(
        "GET /userinfo HTTP/1.1",
        json!({"sub": "test-subject", "name": "Late provider name"}),
        "200 OK",
        Duration::from_millis(300),
    );
    server.reply("GET /jwks HTTP/1.1", serde_json::to_value(keys()).unwrap());
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        provider.userinfo_with_timeout(&tokens, Duration::from_millis(50)),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(crate::ProviderError::Request)));
    server.finish().await;
}

#[test]
fn rotation_and_scope_failures_keep_their_indeterminate_semantics() {
    let now = chrono::Utc::now().timestamp();
    let parse = |value| serde_json::from_value::<super::tokens::Response>(value).unwrap();
    let check = |value, old| {
        Tokens::from_response(
            parse(value),
            &config("https://issuer.example").required_scopes,
            now,
            now,
            old,
            super::id_token::NonceContext::Refresh(None),
        )
    };
    let unchanged = check(response(Value::Null, "same"), Some("refresh-same")).unwrap_err();
    assert_eq!(
        unchanged,
        super::TokenValidationError::UnchangedRefreshToken
    );
    assert_eq!(
        TokenError::InvalidTokens(unchanged).kind(),
        FailureKind::Indeterminate
    );
    let mut missing_scope = response(Value::Null, "new");
    missing_scope["scope"] = json!("openid");
    assert_eq!(
        check(missing_scope, None).unwrap_err(),
        super::TokenValidationError::MissingScope
    );
    let tokens = check(response(Value::Null, "new"), None).unwrap();
    assert_eq!(tokens.expires_at(), now + 300);
    assert_eq!(tokens.refresh_not_before(), Some(now + 241));
}
