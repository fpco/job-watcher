use anyhow::{Result, bail};
use jiff::Zoned;
use job_watcher::axum::{Router, extract::State, routing::get};
use job_watcher::slack::SlackConfig;
use job_watcher::{
    Heartbeat, TaskLabel, WatchedTask, WatchedTaskOutput, WatcherAppContext, WatcherBuilder,
    config::{Delay, TaskConfig, WatcherConfig},
};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    DummyApp::start().await
}

struct LeaderBoard;
struct TaskTwo;

#[derive(Clone)]
struct DummyApp(Zoned);

impl DummyApp {
    async fn start() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:8080").await?;
        println!("Starting leaderboard watcher example.");
        println!("The watcher will run, but the status page is not served in this example.");

        let app = Arc::new(DummyApp(Zoned::now()));
        let mut builder = WatcherBuilder::new(app.clone());

        builder.watch_periodic(TaskLabel::new("leaderboard"), LeaderBoard)?;
        builder.watch_periodic(TaskLabel::new("task_two"), TaskTwo)?;
        builder.watch_background_with_status(TaskLabel::new("task_three"), run_task_three)?;
        builder.watch_background_with_status(TaskLabel::new("task_four"), TaskFour::start)?;

        let task_five = TaskFive(3);
        builder.watch_background_with_status(
            TaskLabel::new("task_five"),
            move |app, heartbeat| async move { task_five.start(app, heartbeat).await },
        )?;

        builder.wait(listener).await
    }
}

impl WatcherAppContext for DummyApp {
    fn environment(&self) -> Option<String> {
        Some("test-env".to_string())
    }

    fn watcher_config(&self) -> WatcherConfig {
        let mut config = WatcherConfig::default();
        config.retries = 0;
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
                retries: Some(3),
                delay_between_retries: None,
            },
        );
        config
    }

    fn triggers_alert(&self, label: &TaskLabel, _selected_label: Option<&TaskLabel>) -> bool {
        // In a real app, you might have different logic for different tasks.
        label.ident() != "leaderboard"
    }

    fn show_output(&self, _label: &TaskLabel) -> bool {
        true
    }

    fn build_version(&self) -> Option<String> {
        Some("dfa2testdkafjakfjakjkafjkafjakfjkajkajkajk".to_owned())
    }

    fn title(&self) -> String {
        "Example application Status".to_owned()
    }

    fn notifier_config(&self) -> Option<job_watcher::NotifierConfig> {
        let webhook = std::env::var("HEALTH_CHECK_SLACK_WEBHOOK").ok();
        webhook.map(|hook| job_watcher::NotifierConfig::Slack(SlackConfig { webhook_url: hook }))
    }

    fn extend_router<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        let custom_router = Router::new()
            .route("/", get(hello_handler))
            .with_state(HelloState(self.0.clone()));

        router.nest_service("/hello", custom_router)
    }
}

#[derive(Clone)]
struct HelloState(Zoned);

async fn hello_handler(State(state): State<HelloState>) -> String {
    format!("Hello from custom route! App live since {}", state.0)
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
        _heartbeat: Heartbeat,
    ) -> Result<WatchedTaskOutput> {
        let random = rand::random_bool(0.5);

        if random {
            bail!("Some error from server")
        } else {
            Ok(WatchedTaskOutput::new("Now succcess"))
        }
    }
}

async fn run_task_three(_app: Arc<DummyApp>, heartbeat: Heartbeat) -> Result<Infallible> {
    let mut i = 1;
    loop {
        println!("task three");
        heartbeat
            .set_status(format!("Status from task three {i}"))
            .await;
        i += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

struct TaskFour;

impl TaskFour {
    pub async fn start(_app: Arc<DummyApp>, heartbeat: Heartbeat) -> Result<Infallible> {
        let mut i = 1;
        loop {
            println!("task four");
            heartbeat
                .set_status(format!("Status from task four {i}"))
                .await;
            i += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

struct TaskFive(pub u64);

impl TaskFive {
    pub async fn start(&self, _app: Arc<DummyApp>, heartbeat: Heartbeat) -> Result<Infallible> {
        let mut i = self.0;
        loop {
            println!("task five");
            heartbeat
                .set_status(format!("Status from task five {i}"))
                .await;
            i += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
