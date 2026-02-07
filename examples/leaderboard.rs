use anyhow::{Result, bail};
use jiff::Zoned;
use job_watcher::{
    AppBuilder, Heartbeat, TaskLabel, WatchedTask, WatchedTaskOutput, WatcherAppContext,
    config::{Delay, TaskConfig, WatcherConfig},
};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    DummyApp::start().await
}

struct LeaderBoard;
struct TaskTwo;

#[derive(Clone)]
struct DummyApp;

impl DummyApp {
    async fn start() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:8080").await?;
        println!("Starting leaderboard watcher example.");
        println!("The watcher will run, but the status page is not served in this example.");

        let app = Arc::new(DummyApp);
        let mut builder = AppBuilder::new(app.clone());

        builder.watch_periodic(TaskLabel::new("leaderboard"), LeaderBoard)?;
        builder.watch_periodic(TaskLabel::new("task_two"), TaskTwo)?;

        builder.wait(listener).await
    }
}

impl WatcherAppContext for DummyApp {
    fn environment(&self) -> Option<String> {
        Some("test-env".to_string())
    }

    fn live_since(&self) -> Zoned {
        Zoned::now()
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
