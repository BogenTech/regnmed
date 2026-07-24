//! Shared harness for the API integration tests: a local "IdP" (fresh
//! RSA key published as a JWKS, exactly as a real issuer would) and app
//! state against the dev database. Requires DATABASE_URL (tests skip
//! otherwise).
#![allow(dead_code)]

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use serde_json::json;
use uuid::Uuid;

use regnmed_api::AppState;
use regnmed_api::auth::Verifier;

pub const ISSUER: &str = "https://id.test.invalid";
pub const AUDIENCE: &str = "regnmed";
pub const KID: &str = "test-key-1";

pub struct TestIdp {
    pub encoding_key: EncodingKey,
    pub jwks: JwkSet,
}

impl TestIdp {
    pub fn new() -> Self {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("generate RSA key");
        let pem = private
            .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .expect("encode PEM");
        let encoding_key = EncodingKey::from_rsa_pem(pem.as_bytes()).expect("load PEM");

        let jwks: JwkSet = serde_json::from_value(json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": KID,
                "n": URL_SAFE_NO_PAD.encode(private.n().to_bytes_be()),
                "e": URL_SAFE_NO_PAD.encode(private.e().to_bytes_be()),
            }]
        }))
        .expect("build JWKS");

        Self { encoding_key, jwks }
    }

    pub fn token(&self, sub: &str, name: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KID.to_string());
        let exp = chrono::Utc::now().timestamp() + 3600;
        let claims = json!({
            "iss": ISSUER,
            "aud": AUDIENCE,
            "sub": sub,
            "name": name,
            "email": format!("{}@test.invalid", sub.replace('|', ".")),
            "exp": exp,
        });
        encode(&header, &claims, &self.encoding_key).expect("sign token")
    }
}

pub async fn test_state(idp: &TestIdp) -> Option<AppState> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    let pool = regnmed_db::connect(&url).await.expect("connect to dev db");
    regnmed_db::MIGRATOR.run(&pool).await.expect("migrate");

    let verifier = Verifier::from_jwks(ISSUER, Some(AUDIENCE.into()), idp.jwks.clone());
    // The mail rail is exercised by tests that spawn their own
    // nats-server and rebuild the state with a connected context.
    Some(AppState {
        pool,
        verifier: Arc::new(verifier),
        mailq: None,
    })
}

pub fn unique_orgnr() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}
