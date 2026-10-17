use super::*;
use crate::client::tests::{claims, config, keys, sign};
use serde_json::json;

const ISSUER: &str = "https://issuer.example";

fn verification() -> Verification {
    Verification {
        jwks_uri: Url::parse("https://issuer.example/jwks").unwrap(),
        algorithms: vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
    }
}

fn name(snapshot: &IdTokenSnapshot, subject: &str, access: &str) -> Option<String> {
    snapshot.verified_name(
        &snapshot.parse()?,
        &config(ISSUER),
        &verification(),
        keys(),
        access,
        subject,
    )
}

fn snapshot(claims: &Value) -> IdTokenSnapshot {
    IdTokenSnapshot::new(Some(sign(claims)), NonceContext::Code("test-nonce".into()))
}

#[test]
fn signatures_are_rechecked_after_storage_and_bound_to_fresh_subject() {
    let original = claims(ISSUER);
    let signed = snapshot(&original);
    let stored = serde_json::to_string(&signed).unwrap();
    let restored: IdTokenSnapshot = serde_json::from_str(&stored).unwrap();
    assert_eq!(
        name(&restored, "test-subject", "access"),
        Some("Fixture ✨".into())
    );
    assert_eq!(name(&restored, "another-subject", "access"), None);
    assert!(!format!("{restored:?}").contains("test-nonce"));
    assert!(!format!("{restored:?}").contains(restored.token.as_ref().unwrap()));

    let mut altered = original;
    altered["custom"]["display_name"] = json!("Tampered");
    let altered_signed = sign(&altered);
    let unsigned = altered_signed.rsplit_once('.').unwrap().0;
    let signature = signed.token.as_ref().unwrap().rsplit_once('.').unwrap().1;
    let tampered = IdTokenSnapshot::new(
        Some(format!("{unsigned}.{signature}")),
        signed.nonce.clone(),
    );
    assert!(tampered.parse().is_some());
    assert_eq!(name(&tampered, "test-subject", "access"), None);

    let no_keys = serde_json::from_value(json!({"keys": []})).unwrap();
    assert_eq!(
        signed.verified_name(
            &signed.parse().unwrap(),
            &config(ISSUER),
            &verification(),
            no_keys,
            "access",
            "test-subject"
        ),
        None
    );
}

#[test]
fn security_claims_are_required_and_checked_not_just_deserialized() {
    let original = claims(ISSUER);
    let now = chrono::Utc::now().timestamp();
    for (field, value) in [
        ("iss", json!("https://attacker.example")),
        ("aud", json!("task-api")),
        ("aud", json!(["subroutine-desktop", "untrusted-audience"])),
        ("azp", json!("other-client")),
        ("exp", json!(now - 1)),
        ("iat", json!(now + 120)),
        ("iat", json!(0)),
        ("sub", json!("")),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        assert_eq!(
            name(&snapshot(&invalid), "test-subject", "access"),
            None,
            "{field}"
        );
    }
    for field in ["iss", "aud", "exp", "iat", "sub"] {
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert_eq!(
            name(&snapshot(&missing), "test-subject", "access"),
            None,
            "missing {field}"
        );
    }
    let mut no_azp = original;
    no_azp.as_object_mut().unwrap().remove("azp");
    assert_eq!(
        name(&snapshot(&no_azp), "test-subject", "access"),
        Some("Fixture ✨".into())
    );
}

#[test]
fn nonce_is_required_for_code_and_checked_if_returned_on_refresh() {
    let mut identity = claims(ISSUER);
    identity["nonce"] = json!("wrong-nonce");
    assert_eq!(name(&snapshot(&identity), "test-subject", "access"), None);
    identity.as_object_mut().unwrap().remove("nonce");
    assert_eq!(name(&snapshot(&identity), "test-subject", "access"), None);
    let mut refreshed = IdTokenSnapshot::new(
        Some(sign(&identity)),
        NonceContext::Refresh(Some("test-nonce".into())),
    );
    assert_eq!(
        name(&refreshed, "test-subject", "access"),
        Some("Fixture ✨".into())
    );
    identity["nonce"] = json!("test-nonce");
    refreshed.token = Some(sign(&identity));
    assert_eq!(
        name(&refreshed, "test-subject", "access"),
        Some("Fixture ✨".into())
    );
    refreshed.nonce = NonceContext::Refresh(Some("wrong-nonce".into()));
    assert_eq!(name(&refreshed, "test-subject", "access"), None);
    refreshed.nonce = NonceContext::Refresh(None);
    assert_eq!(name(&refreshed, "test-subject", "access"), None);
    identity.as_object_mut().unwrap().remove("nonce");
    refreshed.token = Some(sign(&identity));
    assert_eq!(
        name(&refreshed, "test-subject", "access"),
        Some("Fixture ✨".into())
    );
}

#[test]
fn access_token_hash_is_validated_when_present() {
    let mut identity = claims(ISSUER);
    let keys = keys();
    identity["at_hash"] = json!(
        AccessTokenHash::from_token(
            &AccessToken::new("bound-access".into()),
            &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            &keys.keys()[0],
        )
        .unwrap()
    );
    let token = snapshot(&identity);
    assert_eq!(
        name(&token, "test-subject", "bound-access"),
        Some("Fixture ✨".into())
    );
    assert_eq!(name(&token, "test-subject", "other-access"), None);
}

#[test]
fn display_metadata_is_optional_and_malformed_shapes_only_remove_the_override() {
    let mut identity = claims(ISSUER);
    for custom in [
        Value::Null,
        json!([]),
        json!("name"),
        json!(42),
        json!({}),
        json!({"display_name": false}),
        json!({"display_name": {"value": "bad"}}),
    ] {
        identity["custom"] = custom;
        let token = snapshot(&identity);
        assert!(token.parse().is_some());
        assert_eq!(name(&token, "test-subject", "access"), None);
    }
    identity.as_object_mut().unwrap().remove("custom");
    assert_eq!(name(&snapshot(&identity), "test-subject", "access"), None);
}

#[test]
fn freeform_names_follow_scalar_and_unsafe_character_rules() {
    assert_eq!(
        display_name(&json!(" \u{feff}O’Name / ✨\u{a0}")),
        Some("O’Name / ✨".into())
    );
    assert_eq!(display_name(&json!("🦀".repeat(80))), Some("🦀".repeat(80)));
    for value in [
        json!("🦀".repeat(81)),
        json!(format!(" {} ", "x".repeat(80))),
        json!("  "),
        json!("\nName"),
        json!("Name\t"),
        json!("N\u{7f}ame"),
        json!("N\u{85}ame"),
        json!("Name\u{2028}"),
        json!("\u{2029}Name"),
        json!(0),
    ] {
        assert_eq!(display_name(&value), None);
    }
}

#[test]
fn discovery_never_uses_token_provided_keys_or_symmetric_algorithms() {
    let mut metadata = json!({
        "issuer": ISSUER, "authorization_endpoint": "https://issuer.example/authorize",
        "token_endpoint": "https://issuer.example/token", "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"], "jwks_uri": "https://issuer.example/jwks",
        "id_token_signing_alg_values_supported": ["HS256", "none", "RS256", "unknown"],
    });
    let parse = |value| serde_json::from_value::<Discovery>(value).unwrap();
    let trusted = Verification::from_discovery(&config(ISSUER), &parse(metadata.clone())).unwrap();
    assert_eq!(
        trusted.algorithms,
        vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256]
    );
    metadata["jwks_uri"] = json!("https://attacker.example/jwks");
    assert!(Verification::from_discovery(&config(ISSUER), &parse(metadata.clone())).is_none());
    metadata["jwks_uri"] = json!("https://issuer.example/jwks");
    metadata["id_token_signing_alg_values_supported"] = json!(["HS256", "none"]);
    assert!(Verification::from_discovery(&config(ISSUER), &parse(metadata)).is_none());
}
