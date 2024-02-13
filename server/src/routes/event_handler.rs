use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, routing::any, Router};

use axum_core::extract::FromRef;
use orion::hazardous::mac::hmac::sha256::SecretKey;

use crate::config::GitHubAppConfiguration;

use self::{extractors::GitHubEvent, remote::ApplicationAuthentication};

mod extractors;
mod remote;

pub fn router(config: GitHubAppConfiguration) -> Result<Router, Box<dyn std::error::Error>> {
    // FIXME: should I move the remote_config outside??
    let remote_config = remote::authenticate(config.app_identifier, config.app_key)?;
    let signature_config = ConfigState {
        webhook_secret: config.webhook_secret.into(),
        client: remote_config,
    };
    Ok(Router::new().route(
        "/event_handler",
        any(handle_github_event).with_state(signature_config),
    ))
}

#[derive(Clone)]
struct ConfigState {
    webhook_secret: Arc<SecretKey>,
    client: ApplicationAuthentication,
}

impl FromRef<ConfigState> for Arc<SecretKey> {
    fn from_ref(input: &ConfigState) -> Self {
        input.webhook_secret.clone()
    }
}

impl FromRef<ConfigState> for ApplicationAuthentication {
    fn from_ref(input: &ConfigState) -> Self {
        input.client.clone()
    }
}

async fn handle_github_event(
    State(ApplicationAuthentication { client }): State<ApplicationAuthentication>,
    GitHubEvent(event): GitHubEvent,
) -> impl IntoResponse {
    tracing::error!(?client, kind = ?event, "logic starts now");
    if let Some(t) = event.installation {
        let id = match t {
            octocrab::models::webhook_events::EventInstallation::Full(install) => install.id,
            octocrab::models::webhook_events::EventInstallation::Minimal(mini) => mini.id,
        };
        client.installation(id);
    }
    "hello world"
}

#[cfg(test)]
mod test {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use hyper::StatusCode;
    use orion::hazardous::mac::hmac::sha256::{HmacSha256, SecretKey};
    use rsa::RsaPublicKey;
    use serde_json::json;
    use tower::ServiceExt;

    use crate::config::GitHubAppConfiguration;

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_happy_path() {
        let (config, _, secret) = create_test_config();
        let app = super::router(config).unwrap();

        let body = serde_json::to_vec(&json!({"hello": "world"})).unwrap();
        let body_hmac = calc_hmac_for_body(&secret, &body);
        let request = Request::builder()
            .uri("/event_handler")
            .header("X-GitHub-Event", "pull_request.*")
            .header("x-hub-signature-256", format!("sha256={body_hmac}"))
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes();
        // let body: serde_json::Value = str::get(&body).unwrap();
        tracing::info!(?body);
        assert_eq!(parts.status, StatusCode::OK);
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_missing_signature() {
        let (config, _, _) = create_test_config();
        let app = super::router(config).unwrap();

        let body = serde_json::to_vec(&json!({"hello": "world"})).unwrap();
        let request = Request::builder()
            .uri("/event_handler")
            .header("X-GitHub-Event", "pull_request.*")
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_wrong_signature() {
        let (config, _, _) = create_test_config();
        let app = super::router(config).unwrap();

        let body = serde_json::to_vec(&json!({"hello": "world"})).unwrap();
        let request = Request::builder()
            .uri("/event_handler")
            .header("X-GitHub-Event", "pull_request.*")
            .header(
                "x-hub-signature-256",
                "sha256=46288437613044114D21E7FAD79837C12336202F4C85008548FB226693426F56",
            )
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    fn create_test_config() -> (GitHubAppConfiguration, RsaPublicKey, SecretKey) {
        use jsonwebtoken::EncodingKey;
        use octocrab::models::AppId;
        use rand::SeedableRng;
        use rsa::pkcs8::EncodePrivateKey;

        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(17_832_551);
        let bits = 256;
        let priv_key = rsa::RsaPrivateKey::new(&mut rng, bits).expect("failed to generate a key");
        let pub_key = rsa::RsaPublicKey::from(&priv_key);

        let der_encoded_key = priv_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let cert_pem_str = der_encoded_key.to_string();

        let secret = SecretKey::from_slice(&[0; 32]).unwrap();

        (
            GitHubAppConfiguration {
                webhook_secret: secret,
                app_identifier: AppId(1),
                app_key: { EncodingKey::from_rsa_pem(cert_pem_str.as_bytes()).unwrap() },
            },
            pub_key,
            SecretKey::from_slice(&[0; 32]).unwrap(),
        )
    }

    fn calc_hmac_for_body(secret: &SecretKey, data: &[u8]) -> String {
        hex::encode(
            HmacSha256::hmac(&secret, data)
                .unwrap()
                .unprotected_as_bytes(),
        )
    }
}
