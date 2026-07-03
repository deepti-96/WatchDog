use crate::alert;
use crate::engine::WatchdogEngine;
use crate::export;
use crate::llm;
use crate::model::{normalize_incident_status, DeployEvent, Incident, LogEvent, MetricSample};
use crate::storage;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Clone)]
struct AppState {
    state_dir: Arc<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ExplainResponse {
    explanation: String,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    incident_count: usize,
    storage_backend: &'static str,
    state_dir: String,
    explainer: String,
}

#[derive(Debug, Deserialize)]
struct DemoScenarioRequest {
    scenario: Option<String>,
}

#[derive(Debug, Serialize)]
struct DemoScenarioResponse {
    scenario: String,
    incident_id: String,
    incident: Incident,
}

#[derive(Debug, Deserialize)]
struct UpdateStatusRequest {
    status: String,
}

#[derive(Debug, Deserialize)]
struct UpdateNotesRequest {
    notes: String,
}

pub async fn serve(state_dir: PathBuf, host: String, port: u16) -> anyhow::Result<()> {
    let app_state = AppState {
        state_dir: Arc::new(state_dir),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/api/incidents", get(list_incidents))
        .route("/api/demo/scenarios", post(create_demo_scenario))
        .route("/api/incidents/{id}", get(get_incident))
        .route("/api/incidents/{id}/status", post(update_incident_status))
        .route("/api/incidents/{id}/notes", post(update_incident_notes))
        .route("/api/incidents/{id}/explain", post(explain_incident))
        .route(
            "/api/incidents/{id}/explain/refresh",
            post(refresh_incident_explanation),
        )
        .route("/api/incidents/{id}/export/json", get(export_incident_json))
        .route(
            "/api/incidents/{id}/export/markdown",
            get(export_incident_markdown),
        )
        .route("/api/incidents/{id}/summary", get(get_incident_summary))
        .with_state(app_state);

    let address: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = TcpListener::bind(address).await?;
    println!("watchdog dashboard available at http://{}", address);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    let incident_count = storage::list_incidents(&state.state_dir)
        .map(|incidents| incidents.len())
        .unwrap_or_default();

    Json(HealthResponse {
        status: "ok",
        incident_count,
        storage_backend: storage::storage_backend_label(),
        state_dir: state.state_dir.display().to_string(),
        explainer: std::env::var("WATCHDOG_EXPLAINER").unwrap_or_else(|_| "auto".to_string()),
    })
}

async fn list_incidents(State(state): State<AppState>) -> impl IntoResponse {
    match storage::list_incidents(&state.state_dir) {
        Ok(incidents) => Json(
            incidents
                .into_iter()
                .map(|incident| incident.list_item())
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn create_demo_scenario(
    State(state): State<AppState>,
    Json(payload): Json<DemoScenarioRequest>,
) -> Response {
    let scenario = payload
        .scenario
        .unwrap_or_else(|| "checkout-timeout".to_string());

    match run_demo_scenario(&state.state_dir, &scenario) {
        Ok(incident) => Json(DemoScenarioResponse {
            scenario,
            incident_id: incident.id.clone(),
            incident,
        })
        .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_incident(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => Json(incident).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn export_incident_json(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    match serde_json::to_string_pretty(&incident) {
        Ok(body) => {
            let mut response = body.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/json; charset=utf-8"),
            );
            if let Ok(value) = header::HeaderValue::from_str(&format!(
                "attachment; filename=\"{}-incident.json\"",
                incident.id
            )) {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn export_incident_markdown(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    let body = export::render_markdown(&incident);
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    if let Ok(value) = header::HeaderValue::from_str(&format!(
        "attachment; filename=\"{}-incident.md\"",
        incident.id
    )) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

async fn get_incident_summary(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    export::render_summary(&incident).into_response()
}

async fn update_incident_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateStatusRequest>,
) -> impl IntoResponse {
    let Some(status) = normalize_incident_status(&payload.status) else {
        return (StatusCode::BAD_REQUEST, "invalid incident status").into_response();
    };

    match storage::update_incident_status(&state.state_dir, &id, status) {
        Ok(Some(incident)) => Json(incident).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn update_incident_notes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateNotesRequest>,
) -> impl IntoResponse {
    match storage::update_incident_notes(&state.state_dir, &id, &payload.notes) {
        Ok(Some(incident)) => Json(incident).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn refresh_incident_explanation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    generate_incident_explanation(state, id, true).await
}

async fn explain_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    generate_incident_explanation(state, id, false).await
}

async fn generate_incident_explanation(
    state: AppState,
    id: String,
    force_refresh: bool,
) -> Response {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    if !force_refresh {
        if let Some(explanation) = incident.cached_explanation.clone() {
            return Json(ExplainResponse { explanation }).into_response();
        }
    }

    match llm::explain_incident(&incident).await {
        Ok(explanation) => {
            match storage::update_incident_explanation(&state.state_dir, &incident.id, &explanation)
            {
                Ok(Some(updated)) => Json(ExplainResponse {
                    explanation: updated.cached_explanation.unwrap_or(explanation),
                })
                .into_response(),
                Ok(None) => Json(ExplainResponse { explanation }).into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Err(error) => (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}

fn run_demo_scenario(state_dir: &std::path::Path, scenario: &str) -> anyhow::Result<Incident> {
    let mut engine = WatchdogEngine::new(120, 300);
    let now = Utc::now();
    let deploy_id = format!("demo-{}", now.timestamp_millis());
    let environment = match scenario {
        "payments-latency" => "payments",
        _ => "checkout",
    }
    .to_string();

    for i in 0..30 {
        engine.ingest_metric(MetricSample {
            timestamp: now + Duration::seconds(i),
            error_rate: 0.010 + ((i % 3) as f64 * 0.002),
            p95_latency_ms: 112.0 + ((i % 4) as f64 * 4.0),
            request_rate: 390.0 + ((i % 5) as f64 * 6.0),
        });
    }

    let deploy = DeployEvent {
        timestamp: now + Duration::seconds(31),
        deploy_id,
        environment,
    };

    if !engine.mark_deploy(deploy.clone()) {
        anyhow::bail!("demo scenario could not arm deploy correlation");
    }

    for i in 32..48 {
        let timestamp = now + Duration::seconds(i);
        let degraded = i >= 35;
        let (error_rate, latency, signature) = match scenario {
            "payments-latency" => (
                if degraded {
                    0.045 + ((i % 2) as f64 * 0.006)
                } else {
                    0.012
                },
                if degraded {
                    345.0 + ((i % 3) as f64 * 24.0)
                } else {
                    124.0
                },
                "Payment provider timeout while authorizing card 4242 request 8f91ab22",
            ),
            _ => (
                if degraded {
                    0.108 + ((i % 3) as f64 * 0.01)
                } else {
                    0.013
                },
                if degraded {
                    265.0 + ((i % 2) as f64 * 28.0)
                } else {
                    121.0
                },
                "Database timeout while loading checkout session user 123 request 8f91ab22",
            ),
        };

        if degraded {
            engine.ingest_log(LogEvent {
                timestamp,
                level: "ERROR".to_string(),
                service: "api".to_string(),
                message: signature.to_string(),
            });
        }

        if let Some(verdict) = engine.ingest_metric(MetricSample {
            timestamp,
            error_rate,
            p95_latency_ms: latency,
            request_rate: 405.0,
        }) {
            let message = alert::render(&verdict);
            return storage::persist_incident(state_dir, &verdict, &message);
        }
    }

    anyhow::bail!("demo scenario did not produce a regression incident")
}

const INDEX_HTML: &str = include_str!("dashboard.html");
