use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use job_watcher::{
    validators, AppBuilder, Heartbeat, TaskLabel, WatchedTask, WatchedTaskOutput, WatcherAppContext,
    config::{Delay, TaskConfig, ValidatorConfig, WatcherConfig},
};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    DummyApp::start().await
}

struct LeaderBoard;
struct TaskTwo;
struct ValidatorMonitor {
    validator_configs: Vec<ValidatorConfig>,
    registry: validators::ValidatorRegistry,
}

#[derive(Clone)]
struct DummyApp;

impl DummyApp {
    async fn start() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:8080").await?;
        println!("Starting leaderboard watcher example.");
        println!("The watcher will run, with status page at http://127.0.0.1:8080/status");
        println!("Validator status available at http://127.0.0.1:8080/validators");

        let app = Arc::new(DummyApp);
        let mut builder = AppBuilder::new(app.clone());

        builder.watch_periodic(TaskLabel::new("leaderboard"), LeaderBoard)?;
        builder.watch_periodic(TaskLabel::new("task_two"), TaskTwo)?;

        // Example: Add validator monitoring if validators are configured
        let validator_configs = vec![
            // Example validator configuration - uncomment and adjust URLs for real validators
            // ValidatorConfig {
            //     name: "validator-1".to_string(),
            //     url: "http://localhost:3000".to_string(),
            //     public_key: Some("processor_public_key_here".to_string()),
            // },
        ];

        if !validator_configs.is_empty() {
            let registry = builder.validator_registry().clone();
            builder.watch_periodic(
                TaskLabel::new("validator_monitor"),
                ValidatorMonitor {
                    validator_configs,
                    registry,
                },
            )?;
        }

        builder.wait(listener).await
    }
}

impl WatcherAppContext for DummyApp {
    fn environment(&self) -> Option<String> {
        Some("test-env".to_string())
    }

    fn live_since(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn watcher_config(&self) -> WatcherConfig {
        let mut config = WatcherConfig::default();
        config.tasks.insert(
            "leaderboard".to_string(),
            TaskConfig {
                delay: Delay::ConstantSecs(10),
                out_of_date: Some(30),
                retries: None,
                delay_between_retries: None,
            },
        );
        config.tasks.insert(
            "task_two".to_string(),
            TaskConfig {
                delay: Delay::ConstantSecs(1),
                out_of_date: Some(30),
                retries: None,
                delay_between_retries: None,
            },
        );
        config.tasks.insert(
            "validator_monitor".to_string(),
            TaskConfig {
                delay: Delay::ConstantSecs(30),
                out_of_date: Some(60),
                retries: None,
                delay_between_retries: None,
            },
        );
        config
    }

    fn triggers_alert(&self, label: &TaskLabel, _selected_label: Option<&TaskLabel>) -> bool {
        // In a real app, you might have different logic for different tasks.
        if label.ident() == "leaderboard" {
            false
        } else {
            true
        }
    }

    fn show_output(&self, _label: &TaskLabel) -> bool {
        true
    }

    fn build_version(&self) -> Option<String> {
        Some("dfa2test".to_owned())
    }

    fn title(&self) -> String {
        "Example application Status".to_owned()
    }
}

impl WatchedTask<DummyApp> for LeaderBoard {
    async fn run_single(
        &mut self,
        _app: Arc<DummyApp>,
        _heartbeat: Heartbeat,
    ) -> Result<WatchedTaskOutput> {
        update().await
    }
}

async fn update() -> Result<WatchedTaskOutput> {
    println!("Executing leaderboard task...");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    println!("Finished executing leaderboard task.");
    Ok(WatchedTaskOutput::new(
        "Finished executing leaderboard task",
    ))
}

impl WatchedTask<DummyApp> for TaskTwo {
    async fn run_single(
        &mut self,
        _app: Arc<DummyApp>,
        heartbeat: Heartbeat,
    ) -> Result<WatchedTaskOutput> {
        let total_success = heartbeat.task_status.read().await.counts.successes;
        if total_success > 3 {
            println!("Skipping execution of task two");
            bail!("Skipping!")
        } else {
            update_task_two().await
        }
    }
}

async fn update_task_two() -> Result<WatchedTaskOutput> {
    println!("Executing task two...");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    println!("Finished executing task two.");
    Ok(WatchedTaskOutput::new("Finished executing task two"))
}

impl WatchedTask<DummyApp> for ValidatorMonitor {
    async fn run_single(
        &mut self,
        _app: Arc<DummyApp>,
        _heartbeat: Heartbeat,
    ) -> Result<WatchedTaskOutput> {
        println!("Checking validator status...");

        let mut validator_infos = Vec::new();
        let mut summary = Vec::new();

        for config in &self.validator_configs {
            let info = validators::fetch_validator_info(config).await;

            if info.is_reachable {
                summary.push(format!(
                    "{}: height={}, code={}, chain={}",
                    info.name,
                    info.current_height.unwrap_or(0),
                    info.code_version.as_deref().unwrap_or("unknown"),
                    info.chain_version.as_deref().unwrap_or("unknown")
                ));
            } else {
                summary.push(format!(
                    "{}: UNREACHABLE - {}",
                    info.name,
                    info.error.as_deref().unwrap_or("unknown error")
                ));
            }

            validator_infos.push(info);
        }

        // Update the registry with the fetched information
        self.registry.update(validator_infos).await;

        let message = if summary.is_empty() {
            "No validators configured".to_string()
        } else {
            format!("Checked {} validator(s): {}", summary.len(), summary.join("; "))
        };

        println!("{}", message);
        Ok(WatchedTaskOutput::new(message))
    }
}
