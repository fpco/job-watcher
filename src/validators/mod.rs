use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::config::ValidatorConfig;

/// Information retrieved from a Kolme validator
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Name/identifier of the validator
    pub name: String,
    /// URL of the validator API
    pub url: String,
    /// Public key of the validator service
    pub public_key: Option<String>,
    /// Current block height the validator is at
    pub current_height: Option<u64>,
    /// Code version deployed (git version)
    pub code_version: Option<String>,
    /// Chain version
    pub chain_version: Option<String>,
    /// Whether the validator is reachable
    pub is_reachable: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Response from Kolme API /basics endpoint
#[derive(Debug, Deserialize)]
struct KolmeBasicsResponse {
    code_version: String,
    chain_version: String,
    next_height: BlockHeight,
}

#[derive(Debug, Deserialize)]
struct BlockHeight {
    #[serde(rename = "0")]
    height: u64,
}

/// Fetch validator information from a Kolme API endpoint
pub async fn fetch_validator_info(config: &ValidatorConfig) -> ValidatorInfo {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match fetch_validator_info_inner(&client, config).await {
        Ok(info) => info,
        Err(e) => ValidatorInfo {
            name: config.name.clone(),
            url: config.url.clone(),
            public_key: config.public_key.clone(),
            current_height: None,
            code_version: None,
            chain_version: None,
            is_reachable: false,
            error: Some(format!("Failed to fetch validator info: {}", e)),
        },
    }
}

async fn fetch_validator_info_inner(
    client: &reqwest::Client,
    config: &ValidatorConfig,
) -> Result<ValidatorInfo> {
    let url = format!("{}/", config.url.trim_end_matches('/'));

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to fetch validator info: HTTP {}",
            response.status()
        );
    }

    let basics: KolmeBasicsResponse = response.json().await?;

    Ok(ValidatorInfo {
        name: config.name.clone(),
        url: config.url.clone(),
        public_key: config.public_key.clone(),
        current_height: Some(basics.next_height.height.saturating_sub(1)),
        code_version: Some(basics.code_version),
        chain_version: Some(basics.chain_version),
        is_reachable: true,
        error: None,
    })
}

/// Storage for validator information
#[derive(Clone, Default)]
pub struct ValidatorRegistry {
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
}

impl ValidatorRegistry {
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn update(&self, info: Vec<ValidatorInfo>) {
        let mut validators = self.validators.write().await;
        *validators = info;
    }

    pub async fn get_all(&self) -> Vec<ValidatorInfo> {
        self.validators.read().await.clone()
    }

    pub async fn get_by_name(&self, name: &str) -> Option<ValidatorInfo> {
        self.validators
            .read()
            .await
            .iter()
            .find(|v| v.name == name)
            .cloned()
    }
}
