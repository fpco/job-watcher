use crate::defaults;
use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Delay {
    NoDelay,
    ConstantSecs(u64),
    ConstantMSecs(u64),
    Random { low: u64, high: u64 },
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TaskConfig {
    /// Delay between runs
    pub delay: Delay,
    /// How many seconds before we should consider the result out of date
    ///
    /// This does not include the delay time
    pub out_of_date: Option<u32>,
    /// How many times to retry before giving up, overriding the general watcher
    /// config
    pub retries: Option<usize>,
    /// How many seconds to delay between retries, overriding the general
    /// watcher config
    pub delay_between_retries: Option<u32>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ValidatorConfig {
    /// Name/identifier for the validator
    pub name: String,
    /// URL of the Kolme API endpoint
    pub url: String,
    /// Public key of the validator (if known)
    pub public_key: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct WatcherConfig {
    /// How many times to retry before giving up
    #[serde(default = "defaults::retries")]
    pub retries: usize,
    /// How many seconds to delay between retries
    #[serde(default = "defaults::delay_between_retries")]
    pub delay_between_retries: u32,
    #[serde(default)]
    pub tasks: HashMap<String, TaskConfig>,
    /// Validators to monitor
    #[serde(default)]
    pub validators: Vec<ValidatorConfig>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            retries: defaults::retries(),
            delay_between_retries: defaults::delay_between_retries(),
            tasks: Default::default(),
            validators: Default::default(),
        }
    }
}
