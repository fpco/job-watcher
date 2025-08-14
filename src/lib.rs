//! A generic library for monitoring long-running background tasks.
//!
//! This crate provides a framework for defining periodic or continuous tasks,
//! running them in the background, and reporting their status through an HTTP API.
//!
//! # Features
//!
//! - Define tasks that implement the `WatchedTask` trait.
//! - Run tasks periodically with various delay strategies.
//! - Automatic retries with configurable backoff.
//! - Status reporting via HTML, JSON, or plain text.
//! - Extensible through the `WatcherAppContext` trait to integrate with your application.
//!
//! # Usage
//!
//! 1. Define a context struct that implements `WatcherAppContext`.
//! 2. Create an `AppBuilder`.
//! 3. Define your tasks as structs that implement `WatchedTask`.
//! 4. Register your tasks with `watch_periodic`.
//! 5. Start the watcher with `wait`.

mod rest_api;

pub mod config;
mod defaults;

use anyhow::{Context, Result};
use axum::{
    http::{self, HeaderValue},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use config::{Delay, WatcherConfig};
use rand::Rng;
use std::fmt::Write;
use std::{borrow::Cow, collections::HashMap, fmt::Display, pin::Pin, sync::Arc, time::Instant};
use tokio::{net::TcpListener, sync::RwLock, task::JoinSet};

pub trait WatcherAppContext {
    fn title(&self) -> String;
    fn environment(&self) -> Option<String>;
    fn build_version(&self) -> Option<String>;
    fn live_since(&self) -> DateTime<Utc>;
    fn watcher_config(&self) -> WatcherConfig;
    fn triggers_alert(&self, label: &TaskLabel, selected_label: Option<&TaskLabel>) -> bool;
    fn show_output(&self, label: &TaskLabel) -> bool;
}

#[derive(
    Clone, PartialEq, Eq, Debug, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TaskLabel(String);

impl TaskLabel {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn ident(&self) -> &str {
        &self.0
    }
}

impl Display for TaskLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

struct ToSpawn {
    future: Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>,
    label: TaskLabel,
}

#[derive(Clone, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum TaskResultValue {
    Ok(Cow<'static, str>),
    Err(String),
    NotYetRun,
}

const NOT_YET_RUN_MESSAGE: &str = "Task has not yet completed a single run";

impl TaskResultValue {
    fn as_str(&self) -> &str {
        match self {
            TaskResultValue::Ok(s) => s,
            TaskResultValue::Err(s) => s,
            TaskResultValue::NotYetRun => NOT_YET_RUN_MESSAGE,
        }
    }
}

#[derive(Clone, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct TaskResult {
    #[serde(skip)]
    pub value: Arc<TaskResultValue>,
    pub updated: DateTime<Utc>,
}

#[derive(Clone, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct TaskError {
    #[serde(skip)]
    pub value: Arc<String>,
    pub updated: DateTime<Utc>,
}

impl TaskResult {
    fn since(&self) -> Since {
        Since(self.updated)
    }
}

impl TaskError {
    fn since(&self) -> Since {
        Since(self.updated)
    }
}

#[derive(Clone, Copy, Default, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct TaskCounts {
    pub successes: usize,
    pub retries: usize,
    pub errors: usize,
}

impl TaskCounts {
    fn total(&self) -> usize {
        self.successes + self.retries + self.errors
    }
}

#[derive(Clone, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct TaskStatus {
    pub last_result: TaskResult,
    pub last_retry_error: Option<TaskError>,
    pub current_run_started: Option<DateTime<Utc>>,
    /// Is the last_result out of date ?
    #[serde(skip)]
    pub out_of_date: Option<Duration>,
    /// Should we expire the status of last result ?
    #[serde(skip)]
    pub expire_last_result: Option<(std::time::Duration, Instant)>,
    pub counts: TaskCounts,
    pub last_run_seconds: Option<i64>,
}

pub(crate) type StatusMap = HashMap<TaskLabel, Arc<RwLock<TaskStatus>>>;

#[derive(Default)]
pub(crate) struct Watcher {
    to_spawn: Vec<ToSpawn>,
    set: JoinSet<Result<()>>,
    statuses: StatusMap,
}

#[derive(Default, Clone)]
pub(crate) struct TaskStatuses {
    statuses: Arc<StatusMap>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
enum ShortStatus {
    Error,
    OutOfDateError,
    OutOfDate,
    ErrorNoAlert,
    OutOfDateNoAlert,
    Success,
    NotYetRun,
}

impl ShortStatus {
    fn as_str(self) -> &'static str {
        match self {
            ShortStatus::OutOfDate => "OUT OF DATE",
            ShortStatus::OutOfDateError => "ERROR DUE TO OUT OF DATE",
            ShortStatus::OutOfDateNoAlert => "OUT OF DATE (no alert)",
            ShortStatus::Success => "SUCCESS",
            ShortStatus::Error => "ERROR",
            ShortStatus::ErrorNoAlert => "ERROR (no alert)",
            ShortStatus::NotYetRun => "NOT YET RUN",
        }
    }

    fn alert(&self) -> bool {
        match self {
            ShortStatus::Error => true,
            ShortStatus::OutOfDateError => true,
            ShortStatus::OutOfDate => false,
            ShortStatus::ErrorNoAlert => false,
            ShortStatus::OutOfDateNoAlert => false,
            ShortStatus::Success => false,
            ShortStatus::NotYetRun => false,
        }
    }

    fn css_class(self) -> &'static str {
        match self {
            ShortStatus::Error => "link-danger",
            ShortStatus::OutOfDateError => "link-danger",
            ShortStatus::OutOfDate => "text-red-400",
            ShortStatus::ErrorNoAlert => "text-red-400",
            ShortStatus::OutOfDateNoAlert => "text-red-300",
            ShortStatus::Success => "link-success",
            ShortStatus::NotYetRun => "link-primary",
        }
    }
}

#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
struct RenderedStatus {
    label: TaskLabel,
    status: TaskStatus,
    short: ShortStatus,
}

use askama::Template;
#[derive(Template, serde::Serialize)]
#[template(path = "status.html")]
#[serde(rename_all = "kebab-case")]
struct StatusTemplate<'a> {
    statuses: Vec<RenderedStatus>,
    env: Cow<'a, str>,
    build_version: Cow<'a, str>,
    live_since: DateTime<Utc>,
    now: DateTime<Utc>,
    alert: bool,
    title: String
}

impl TaskStatuses {
    async fn to_template<'a, C: WatcherAppContext>(
        &'a self,
        app: &'a C,
        label: Option<TaskLabel>,
    ) -> StatusTemplate<'a> {
        let statuses = self.statuses(app, label).await;
        let alert = statuses.iter().any(|x| x.short.alert());

        StatusTemplate {
            statuses,
            env: app.environment().unwrap_or("Unknown".to_owned()).into(),
            build_version: app.build_version().unwrap_or("Unknown".to_owned()).into(),
            now: Utc::now(),
            alert,
            live_since: app.live_since(),
            title: app.title()
        }
    }

    async fn statuses<C: WatcherAppContext>(
        &self,
        app: &C,
        selected_label: Option<TaskLabel>,
    ) -> Vec<RenderedStatus> {
        let mut all_statuses = vec![];
        for (label, status) in self
            .statuses
            .iter()
            .filter(|(curr_label, _)| selected_label.as_ref().is_none_or(|l| *curr_label == l))
        {
            let label = label.clone();
            let status = status.read().await.clone();
            let short = status.short(app, &label, selected_label.as_ref());
            all_statuses.push(RenderedStatus {
                label,
                status,
                short,
            });
        }

        all_statuses.sort_by_key(|x| (x.short, x.label.clone()));
        all_statuses
    }

    pub(crate) async fn statuses_html<C: WatcherAppContext>(
        &self,
        app: &C,
        label: Option<TaskLabel>,
    ) -> axum::response::Response {
        let template = self.to_template(app, label).await;
        let mut res = template.render().unwrap().into_response();
        res.headers_mut().insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );

        if template.alert {
            let failure_status = template
                .statuses
                .iter()
                .filter(|x| x.short.alert())
                .collect::<Vec<_>>();
            tracing::error!("Status failure: {:#?}", failure_status);
            *res.status_mut() = http::status::StatusCode::INTERNAL_SERVER_ERROR;
        }

        res
    }

    pub(crate) async fn statuses_json<C: WatcherAppContext>(
        &self,
        app: &C,
        label: Option<TaskLabel>,
    ) -> axum::response::Response {
        let template = self.to_template(app, label).await;

        let mut res = Json(&template).into_response();

        if template.alert {
            let failure_status = template
                .statuses
                .iter()
                .filter(|x| x.short.alert())
                .collect::<Vec<_>>();
            tracing::error!("Status failure: {:#?}", failure_status);
            *res.status_mut() = http::status::StatusCode::INTERNAL_SERVER_ERROR;
        }

        res
    }

    pub(crate) async fn statuses_text<C: WatcherAppContext>(
        &self,
        app: &C,
        label: Option<TaskLabel>,
    ) -> axum::response::Response {
        let mut response_builder = ResponseBuilder {
            buffer: "".to_owned(),
            any_errors: false,
        };

        let statuses = self.statuses(app, label).await;
        let alert = statuses.iter().any(|x| x.short.alert());

        statuses
            .iter()
            .for_each(|rendered| response_builder.add(rendered.clone()).unwrap());
        let mut res = response_builder.into_response();

        if alert {
            let failure_status = statuses
                .iter()
                .filter(|x| x.short.alert())
                .collect::<Vec<_>>();
            tracing::error!("Status failure: {:#?}", failure_status);
            *res.status_mut() = http::status::StatusCode::INTERNAL_SERVER_ERROR;
        }

        res
    }
}

struct ResponseBuilder {
    buffer: String,
    any_errors: bool,
}

impl ResponseBuilder {
    fn add(
        &mut self,
        RenderedStatus {
            label,
            status:
                TaskStatus {
                    last_result,
                    last_retry_error,
                    current_run_started,
                    out_of_date: _,
                    counts: _,
                    expire_last_result: _,
                    last_run_seconds,
                },
            short,
        }: RenderedStatus,
    ) -> std::fmt::Result {
        writeln!(&mut self.buffer, "# {label}. Status: {}", short.as_str())?;

        if let Some(started) = current_run_started {
            writeln!(&mut self.buffer, "Currently running, started at {started}")?;
        }

        if let Some(secs) = last_run_seconds {
            writeln!(&mut self.buffer, "Last run took {secs} seconds")?;
        }

        writeln!(&mut self.buffer)?;
        match last_result.value.as_ref() {
            TaskResultValue::Ok(msg) => {
                writeln!(&mut self.buffer, "{msg}")?;
            }
            TaskResultValue::Err(err) => {
                writeln!(&mut self.buffer, "{err}")?;
            }
            TaskResultValue::NotYetRun => writeln!(&mut self.buffer, "{}", NOT_YET_RUN_MESSAGE)?,
        }
        writeln!(&mut self.buffer)?;

        if let Some(err) = last_retry_error {
            writeln!(&mut self.buffer)?;
            writeln!(
                &mut self.buffer,
                "Currently retrying, last attempt failed with:\n\n{}",
                err.value
            )?;
            writeln!(&mut self.buffer)?;
        }

        writeln!(&mut self.buffer)?;
        Ok(())
    }

    fn into_response(self) -> axum::response::Response {
        let mut res = self.buffer.into_response();
        if self.any_errors {
            *res.status_mut() = http::status::StatusCode::INTERNAL_SERVER_ERROR;
        }
        res
    }
}

enum OutOfDateType {
    Not,
    Slightly,
    Very,
}

impl TaskStatus {
    fn total_run_time(&self) -> String {
        match self.last_run_seconds {
            Some(secs) => secs.to_string(),
            None => "Unknown".to_owned(),
        }
    }

    fn is_expired(&self) -> bool {
        if let Some((expiry_duration, instant)) = self.expire_last_result {
            instant.elapsed() >= expiry_duration
        } else {
            false
        }
    }

    fn is_out_of_date(&self) -> OutOfDateType {
        match self.current_run_started {
            Some(started) => match self.out_of_date {
                Some(out_of_date) => {
                    let now = Utc::now();
                    if started + Duration::seconds(300) <= now {
                        OutOfDateType::Very
                    } else if started + out_of_date <= now {
                        OutOfDateType::Slightly
                    } else {
                        OutOfDateType::Not
                    }
                }
                None => OutOfDateType::Not,
            },
            None => OutOfDateType::Not,
        }
    }

    fn short<C: WatcherAppContext>(
        &self,
        app: &C,
        label: &TaskLabel,
        selected_label: Option<&TaskLabel>,
    ) -> ShortStatus {
        match self.last_result.value.as_ref() {
            TaskResultValue::Ok(_) => {
                match (
                    self.is_out_of_date(),
                    app.triggers_alert(label, selected_label),
                ) {
                    (OutOfDateType::Not, _) => ShortStatus::Success,
                    (_, false) => ShortStatus::OutOfDateNoAlert,
                    (OutOfDateType::Slightly, true) => ShortStatus::OutOfDate,
                    (OutOfDateType::Very, true) => ShortStatus::OutOfDateError,
                }
            }
            TaskResultValue::Err(_) => {
                if app.triggers_alert(label, selected_label) {
                    if self.is_expired() {
                        ShortStatus::ErrorNoAlert
                    } else {
                        ShortStatus::Error
                    }
                } else {
                    ShortStatus::ErrorNoAlert
                }
            }
            TaskResultValue::NotYetRun => ShortStatus::NotYetRun,
        }
    }
}

struct Since(DateTime<Utc>);

impl Display for Since {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let duration = Utc::now().signed_duration_since(self.0);
        let secs = duration.num_seconds();

        match secs.cmp(&0) {
            std::cmp::Ordering::Less => write!(f, "{}", self.0),
            std::cmp::Ordering::Equal => write!(f, "just now ({})", self.0),
            std::cmp::Ordering::Greater => {
                let minutes = secs / 60;
                let secs = secs % 60;
                let hours = minutes / 60;
                let minutes = minutes % 60;
                let days = hours / 24;
                let hours = hours % 24;

                let mut need_space = false;
                for (number, letter) in [(days, 'd'), (hours, 'h'), (minutes, 'm'), (secs, 's')] {
                    if number > 0 {
                        if need_space {
                            write!(f, " {number}{letter}")?;
                        } else {
                            need_space = true;
                            write!(f, "{number}{letter}")?;
                        }
                    }
                }

                write!(f, " ({})", self.0)
            }
        }
    }
}

pub struct AppBuilder<C> {
    pub app: Arc<C>,
    pub(crate) watcher: Watcher,
}

impl Watcher {
    pub async fn wait<C: WatcherAppContext + Send + Sync + Clone + 'static>(
        mut self,
        app: Arc<C>,
        listener: TcpListener,
    ) -> Result<()> {
        self.set.spawn(rest_api::start_rest_api(
            app,
            TaskStatuses {
                statuses: Arc::new(self.statuses),
            },
            listener,
        ));
        for ToSpawn { future, label } in self.to_spawn {
            self.set.spawn(async move {
                future
                    .await
                    .with_context(|| format!("Failure while running: {label}"))
            });
        }
        while let Some(res) = self.set.join_next().await {
            if let Err(e) = res.map_err(|e| e.into()).and_then(|res| res) {
                self.set.abort_all();
                return Err(e);
            }
        }
        Ok(())
    }
}

pub struct Heartbeat {
    pub task_status: Arc<RwLock<TaskStatus>>,
}

#[derive(Debug)]
pub struct WatchedTaskOutput {
    /// Should we skip delay between tasks ? If yes, then we dont
    /// sleep once the task gets completed.
    skip_delay: bool,
    /// Should we supress the output ? If we supress, the new output
    /// won't be reflected. The last_result value will be used instead.
    suppress: bool,
    message: Cow<'static, str>,
    /// Controls the stickiness of this message. After how long should
    /// we treat this as a non alert ?
    expire_alert: Option<(std::time::Duration, Instant)>,
    /// Is the message an error ?
    error: bool,
}

impl WatchedTaskOutput {
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        WatchedTaskOutput {
            skip_delay: false,
            suppress: false,
            message: message.into(),
            expire_alert: None,
            error: false,
        }
    }

    pub fn set_expiry(mut self, expire_duration: std::time::Duration) -> Self {
        self.expire_alert = Some((expire_duration, Instant::now()));
        self
    }

    pub fn skip_delay(mut self) -> Self {
        self.skip_delay = true;
        self
    }

    pub fn set_error(mut self) -> Self {
        self.error = true;
        self
    }
}

const MAX_TASK_SECONDS: u64 = 180;

pub trait WatchedTask<C: Send + Sync + 'static>: Send + Sync + 'static {
    fn run_single(
        &mut self,
        app: Arc<C>,
        heartbeat: Heartbeat,
    ) -> impl std::future::Future<Output = Result<WatchedTaskOutput>> + Send;
    fn run_single_with_timeout(
        &mut self,
        app: Arc<C>,
        heartbeat: Heartbeat,
        should_timeout: bool,
    ) -> impl std::future::Future<Output = Result<WatchedTaskOutput>> + Send {
        async move {
            if should_timeout {
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(MAX_TASK_SECONDS),
                    self.run_single(app, heartbeat),
                )
                .await
                {
                    Ok(x) => x,
                    Err(e) => Err(anyhow::anyhow!(
                        "Running a single task took too long, killing. Elapsed time: {e}"
                    )),
                }
            } else {
                self.run_single(app, heartbeat).await
            }
        }
    }
}

impl<C: WatcherAppContext + Send + Sync + Clone + 'static> AppBuilder<C> {
    pub fn new(app: Arc<C>) -> Self {
        Self {
            app,
            watcher: Default::default(),
        }
    }

    pub async fn wait(self, listener: TcpListener) -> Result<()> {
        self.watcher.wait(self.app, listener).await
    }

    /// Watch a background job that runs continuously, launched immediately
    pub fn watch_background<Fut>(&mut self, task: Fut)
    where
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.watcher.set.spawn(task);
    }

    pub fn watch_periodic<T>(&mut self, label: TaskLabel, mut task: T) -> Result<()>
    where
        T: WatchedTask<C>,
    {
        let task_config_for = |config: &WatcherConfig, label: &TaskLabel| {
            config.tasks.get(label.ident()).copied().unwrap_or_else(|| {
                panic!(
                    "all tasks in TaskLabel must have a config: {}",
                    label.ident()
                )
            })
        };

        let label_clone = label.clone();
        let config = task_config_for(&self.app.watcher_config(), &label);
        let out_of_date = config
            .out_of_date
            .map(|item| chrono::Duration::seconds(item.into()));
        let task_status = Arc::new(RwLock::new(TaskStatus {
            last_result: TaskResult {
                value: TaskResultValue::NotYetRun.into(),
                updated: Utc::now(),
            },
            last_retry_error: None,
            current_run_started: None,
            out_of_date,
            counts: Default::default(),
            expire_last_result: None,
            last_run_seconds: None,
        }));
        {
            let old = self
                .watcher
                .statuses
                .insert(label.clone(), task_status.clone());
            if old.is_some() {
                anyhow::bail!("Two periodic tasks with label {label:?}");
            }
        }
        let app = self.app.clone();
        let future = Box::pin(async move {
            // let mut last_seen_block_height = 0;
            let mut retries = 0;
            loop {
                let old_counts = {
                    let mut guard = task_status.write().await;
                    let old = &*guard;
                    *guard = TaskStatus {
                        last_result: old.last_result.clone(),
                        last_retry_error: old.last_retry_error.clone(),
                        current_run_started: Some(Utc::now()),
                        out_of_date,
                        counts: old.counts,
                        expire_last_result: old.expire_last_result,
                        last_run_seconds: old.last_run_seconds,
                    };
                    guard.counts
                };
                let res = task
                    .run_single_with_timeout(
                        app.clone(),
                        Heartbeat {
                            task_status: task_status.clone(),
                        },
                        out_of_date.is_some(),
                    )
                    .await;
                match res {
                    Ok(WatchedTaskOutput {
                        skip_delay,
                        message,
                        suppress,
                        expire_alert,
                        error,
                    }) => {
                        if app.show_output(&label) {
                            tracing::info!("{label}: Success! {message}");
                        } else {
                            tracing::debug!("{label}: Success! {message}");
                        }
                        {
                            let mut guard = task_status.write().await;
                            let old = &*guard;
                            let _title = label.to_string();
                            if app.triggers_alert(&label, None) {
                                match &*old.last_result.value {
                                    TaskResultValue::Ok(_) => {
                                        if error {
                                            // Was a success, but not a success now
                                            // todo: send pagerduty triggers.
                                        }
                                    }
                                    TaskResultValue::Err(_err) => {
                                        // todo: Send a pager duty trigger with recovered
                                    }
                                    TaskResultValue::NotYetRun => {
                                        // Catalog newly started
                                    }
                                }
                            }
                            let last_run_seconds = {
                                if let Some(old_run_started) = old.current_run_started {
                                    let duration = Utc::now() - old_run_started;
                                    Some(duration.num_seconds())
                                } else {
                                    None
                                }
                            };
                            *guard = TaskStatus {
                                last_result: TaskResult {
                                    value: if suppress {
                                        guard.last_result.value.clone()
                                    } else if error {
                                        TaskResultValue::Err(message.into()).into()
                                    } else {
                                        TaskResultValue::Ok(message).into()
                                    },
                                    updated: Utc::now(),
                                },
                                last_retry_error: None,
                                current_run_started: None,
                                out_of_date,
                                counts: TaskCounts {
                                    successes: if error {
                                        old_counts.successes
                                    } else {
                                        old_counts.successes + 1
                                    },
                                    errors: if error {
                                        old_counts.errors + 1
                                    } else {
                                        old_counts.errors
                                    },
                                    ..old_counts
                                },
                                expire_last_result: expire_alert,
                                last_run_seconds,
                            };
                        }
                        retries = 0;
                        if !skip_delay {
                            match config.delay {
                                Delay::NoDelay => (),
                                Delay::ConstantMSecs(msecs) => {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(msecs))
                                        .await;
                                }
                                Delay::ConstantSecs(secs) => {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(secs))
                                        .await;
                                }
                                Delay::Random { low, high } => {
                                    let secs = rand::rng().random_range(low..=high);
                                    tokio::time::sleep(tokio::time::Duration::from_secs(secs))
                                        .await;
                                }
                            };
                        }
                    }
                    Err(err) => {
                        if app.show_output(&label.clone()) {
                            tracing::warn!("{label}: Error: {err:?}");
                        } else {
                            tracing::debug!("{label}: Error: {err:?}");
                        }
                        retries += 1;
                        let max_retries = config.retries.unwrap_or(app.watcher_config().retries);
                        // We want to get to first failure quickly so we don't
                        // have a misleading success status page. So if this
                        // failed and there are no prior attempts, don't retry.
                        if retries >= max_retries || task_status.read().await.counts.total() == 0 {
                            retries = 0;
                            let mut guard = task_status.write().await;
                            let old = &*guard;
                            let last_run_seconds = {
                                if let Some(old_run_started) = old.current_run_started {
                                    let duration = Utc::now() - old_run_started;
                                    Some(duration.num_seconds())
                                } else {
                                    None
                                }
                            };
                            let _title = label.clone().to_string();
                            let new_error_message = format!("{err:?}");

                            // Sentry/OpsGenie: only send alerts for labels that require it
                            if app.triggers_alert(&label, None) {
                                match &*old.last_result.value {
                                    // The same error is happening as before
                                    TaskResultValue::Err(e) if e == &new_error_message => (),

                                    // Previous state is a different error.
                                    TaskResultValue::Err(_e) => {
                                        // New error occurs.
                                        // todo: send pagerduty trigger
                                    }
                                    // Previous state is either unknown (NotYetRun), or Ok
                                    _ => {
                                        // todo: send pagerduty trigger
                                    }
                                }
                            }
                            *guard = TaskStatus {
                                last_result: TaskResult {
                                    value: TaskResultValue::Err(new_error_message).into(),
                                    updated: Utc::now(),
                                },
                                last_retry_error: None,
                                current_run_started: None,
                                out_of_date,
                                counts: TaskCounts {
                                    errors: old_counts.errors + 1,
                                    ..old_counts
                                },
                                expire_last_result: None,
                                last_run_seconds,
                            };
                        } else {
                            {
                                let mut guard = task_status.write().await;
                                let old = &*guard;
                                let last_run_seconds = {
                                    if let Some(old_run_started) = old.current_run_started {
                                        let duration = Utc::now() - old_run_started;
                                        Some(duration.num_seconds())
                                    } else {
                                        None
                                    }
                                };
                                *guard = TaskStatus {
                                    last_result: old.last_result.clone(),
                                    last_retry_error: Some(TaskError {
                                        value: format!("{err:?}").into(),
                                        updated: Utc::now(),
                                    }),
                                    current_run_started: None,
                                    out_of_date,
                                    counts: TaskCounts {
                                        retries: old_counts.retries + 1,
                                        ..old_counts
                                    },
                                    expire_last_result: None,
                                    last_run_seconds,
                                };
                            }
                        }

                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            config
                                .delay_between_retries
                                .unwrap_or(app.watcher_config().delay_between_retries)
                                .into(),
                        ))
                        .await;
                    }
                }
            }
        });
        self.watcher.to_spawn.push(ToSpawn {
            future,
            label: label_clone,
        });
        Ok(())
    }
}
