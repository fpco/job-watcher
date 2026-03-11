use std::{convert::Infallible, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::{self, TraceLayer},
};
use tracing::Level;

use super::{TaskLabel, TaskStatuses, WatcherAppContext};

#[derive(Clone)]
pub(crate) struct RestApp<C>
where
    C: Clone,
{
    pub(crate) app: Arc<C>,
    pub(crate) statuses: TaskStatuses,
}

pub(crate) async fn start_rest_api<C: WatcherAppContext + Send + Sync + Clone + 'static>(
    app: Arc<C>,
    statuses: TaskStatuses,
    listener: TcpListener,
) -> Result<Infallible> {
    let service_builder = ServiceBuilder::new()
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_request(trace::DefaultOnRequest::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(RequestBodyLimitLayer::new(1_024_000))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(3),
        ));

    let router = axum::Router::new()
        .route("/", get(homepage))
        .route("/healthz", get(healthz))
        .route("/status/{*label}", get(status::single))
        .route("/status", get(status::all));

    let router = app.extend_router(router);

    let router = router.layer(service_builder).with_state(RestApp {
        app: app.clone(),
        statuses,
    });

    tracing::info!("Launching server");

    let future = axum::serve(listener, router.into_make_service());
    future.await?;
    anyhow::bail!("REST API server shut down unexpectedly")
}

pub(crate) async fn healthz() -> &'static str {
    "Yup, I'm healthy!"
}

pub(crate) async fn homepage() -> &'static str {
    "Index page"
}

mod status {
    use super::*;

    async fn handle_status_request<C: WatcherAppContext + Send + Sync + Clone>(
        rest_app: RestApp<C>,
        label: Option<TaskLabel>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        let app = &*rest_app.app;
        let statuses = &rest_app.statuses;

        let accept_header = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok());

        match accept_header {
            Some(accept) if accept.contains("application/json") => {
                statuses.statuses_json(app, label).await
            }
            Some(accept) if accept.contains("text/plain") => {
                statuses.statuses_text(app, label).await
            }
            _ => statuses.statuses_html(app, label).await,
        }
    }

    pub(crate) async fn all<C: WatcherAppContext + Send + Sync + Clone + 'static>(
        State(rest_app): State<RestApp<C>>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        handle_status_request(rest_app, None, headers).await
    }

    pub(crate) async fn single<C: WatcherAppContext + Send + Sync + Clone + 'static>(
        State(rest_app): State<RestApp<C>>,
        Path(label): Path<String>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        handle_status_request(rest_app, Some(TaskLabel::new(label)), headers).await
    }
}
