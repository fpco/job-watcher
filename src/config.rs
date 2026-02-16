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
    /// Delay between successful task executions.
    pub delay: Delay,
    /// Number of seconds a task can run before it is considered "out of date".
    ///
    /// If a task's execution time exceeds this value, its status will be flagged
    /// accordingly. This is useful for monitoring tasks that might be stuck.
    ///
    /// Setting this also enables a hard timeout on task execution. If a task
    /// runs for longer than `MAX_TASK_SECONDS` (180 seconds), it will be
    /// cancelled.
    pub out_of_date: Option<u32>,
    /// Number of times to retry a failing task before giving up.
    ///
    /// This overrides the global `retries` setting in `WatcherConfig`.
    pub retries: Option<usize>,
    /// Delay in seconds between retries of a failing task.
    ///
    /// This overrides the global `delay_between_retries` setting in `WatcherConfig`.
    pub delay_between_retries: Option<u32>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct WatcherConfig {
    /// How many times to retry before giving up
    pub retries: usize,
    /// How many seconds to delay between retries
    pub delay_between_retries: u32,
    pub tasks: HashMap<String, TaskConfig>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            retries: 6,
            delay_between_retries: 20,
            tasks: Default::default(),
        }
    }
}
