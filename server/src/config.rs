use std::sync::Arc;

use envious::EnvDeserializationError;
use jsonwebtoken::EncodingKey;
use octocrab::models::AppId;
use orion::{
    errors::UnknownCryptoError, hazardous::mac::hmac::sha256::SecretKey as Sha256SecretKey,
};
use thiserror::Error;

pub fn load_github_app_config() -> Result<GitHubAppConfiguration, ConfigurationError> {
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    struct ApplicationRawConfig {
        #[serde(default)]
        github_private_key: String,
        #[serde(default)]
        github_webhook_secret: String,
        #[serde(default)]
        github_app_identifier: u64,
    }

    let raw_config: ApplicationRawConfig = {
        let mut env_config = envious::Config::new();
        env_config.case_sensitive(false);
        env_config.build_from_env()?
    };

    let webhook_secret =
        Sha256SecretKey::from_slice(raw_config.github_webhook_secret.as_bytes())?.into();
    let app_identifier = AppId(raw_config.github_app_identifier);
    let app_key = EncodingKey::from_rsa_pem(raw_config.github_private_key.as_bytes())?;

    Ok(GitHubAppConfiguration {
        webhook_secret,
        app_identifier,
        app_key,
    })
}

#[derive(Clone)]
pub struct GitHubAppConfiguration {
    pub webhook_secret: Arc<Sha256SecretKey>,
    pub app_identifier: AppId,
    pub app_key: EncodingKey,
}

#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error("Cannot read from environment configuration")]
    EnvironmentConfigNotReadable(#[from] EnvDeserializationError),
    #[error("Unable to read the cryptocraphical key")]
    UnsupportedCryptography(#[from] UnknownCryptoError),
    #[error("Invalid RSA Key")]
    InvalidRsaError(#[from] jsonwebtoken::errors::Error),
}