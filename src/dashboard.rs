use crate::export;
use crate::llm;
use crate::model::normalize_incident_status;
use crate::storage;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
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
        .route("/api/incidents", get(list_incidents))
        .route("/api/incidents/{id}", get(get_incident))
        .route("/api/incidents/{id}/status", post(update_incident_status))
        .route("/api/incidents/{id}/notes", post(update_incident_notes))
        .route("/api/incidents/{id}/explain", post(explain_incident))
        .route("/api/incidents/{id}/export/json", get(export_incident_json))
        .route("/api/incidents/{id}/export/markdown", get(export_incident_markdown))
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

async fn get_incident(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => Json(incident).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn export_incident_json(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
                response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
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
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
        response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    response
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

async fn explain_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    };

    if let Some(explanation) = incident.cached_explanation.clone() {
        return Json(ExplainResponse { explanation }).into_response();
    }

    match llm::explain_incident(&incident).await {
        Ok(explanation) => match storage::update_incident_explanation(&state.state_dir, &incident.id, &explanation) {
            Ok(Some(updated)) => Json(ExplainResponse {
                explanation: updated.cached_explanation.unwrap_or(explanation),
            })
            .into_response(),
            Ok(None) => Json(ExplainResponse { explanation }).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
        Err(error) => (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>watchdog dashboard</title>
  <style>
    :root {
      --bg: #f5efe4;
      --bg-strong: #fdf9f2;
      --surface: rgba(255, 251, 244, 0.8);
      --surface-strong: rgba(255, 255, 255, 0.96);
      --surface-tint: rgba(244, 236, 223, 0.72);
      --ink: #17202a;
      --muted: #5a6674;
      --line: rgba(180, 158, 129, 0.28);
      --line-strong: rgba(138, 118, 95, 0.38);
      --accent: #0f766e;
      --accent-strong: #0a5b55;
      --accent-soft: rgba(15, 118, 110, 0.11);
      --accent-glow: rgba(15, 118, 110, 0.18);
      --danger: #991b1b;
      --danger-soft: rgba(220, 38, 38, 0.12);
      --warning: #9a3412;
      --warning-soft: rgba(234, 88, 12, 0.12);
      --shadow-lg: 0 28px 80px rgba(23, 32, 42, 0.14);
      --shadow-md: 0 16px 42px rgba(23, 32, 42, 0.1);
      --shadow-sm: 0 10px 24px rgba(23, 32, 42, 0.08);
      --focus: #0b7f74;
      --radius-xl: 30px;
      --radius-lg: 24px;
      --radius-md: 18px;
      --radius-sm: 14px;
      --theme-icon: "🌙";
    }

    html[data-theme="dark"] {
      color-scheme: dark;
      --bg: #0d141a;
      --bg-strong: #121c24;
      --surface: rgba(17, 28, 36, 0.78);
      --surface-strong: rgba(24, 37, 48, 0.96);
      --surface-tint: rgba(18, 30, 39, 0.82);
      --ink: #eef4f7;
      --muted: #9ba9b6;
      --line: rgba(122, 147, 168, 0.22);
      --line-strong: rgba(140, 168, 191, 0.34);
      --accent: #54d0c3;
      --accent-strong: #8be7de;
      --accent-soft: rgba(84, 208, 195, 0.14);
      --accent-glow: rgba(84, 208, 195, 0.2);
      --danger: #ff8f95;
      --danger-soft: rgba(255, 143, 149, 0.13);
      --warning: #ffbb7a;
      --warning-soft: rgba(255, 187, 122, 0.12);
      --shadow-lg: 0 30px 90px rgba(0, 0, 0, 0.42);
      --shadow-md: 0 16px 44px rgba(0, 0, 0, 0.28);
      --shadow-sm: 0 10px 26px rgba(0, 0, 0, 0.2);
      --focus: #7ae8dc;
      --theme-icon: "☀";
    }

    * { box-sizing: border-box; }
    html { color-scheme: light; }

    body {
      margin: 0;
      min-height: 100vh;
      font-family: Georgia, "Times New Roman", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at 0% 0%, rgba(15, 118, 110, 0.16), transparent 24%),
        radial-gradient(circle at 100% 0%, rgba(153, 27, 27, 0.09), transparent 22%),
        linear-gradient(180deg, #fbf6ed 0%, var(--bg) 100%);
      overflow-x: hidden;
      transition: background 220ms ease, color 220ms ease;
    }

    html[data-theme="dark"] body {
      background:
        radial-gradient(circle at 0% 0%, rgba(84, 208, 195, 0.16), transparent 26%),
        radial-gradient(circle at 100% 0%, rgba(255, 143, 149, 0.08), transparent 22%),
        linear-gradient(180deg, #101820 0%, var(--bg) 100%);
    }

    body::before {
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      background:
        linear-gradient(115deg, transparent 0%, rgba(255, 255, 255, 0.26) 45%, transparent 70%),
        radial-gradient(circle at 30% 20%, rgba(255, 255, 255, 0.35), transparent 28%);
      opacity: 0.8;
      transition: opacity 220ms ease;
    }

    html[data-theme="dark"] body::before {
      opacity: 0.26;
      background:
        linear-gradient(115deg, transparent 0%, rgba(255, 255, 255, 0.06) 45%, transparent 72%),
        radial-gradient(circle at 30% 20%, rgba(255, 255, 255, 0.08), transparent 28%);
    }

    button, article, section, input {
      font: inherit;
    }

    button:focus-visible,
    article:focus-visible,
    .refresh-link:focus-visible,
    .theme-toggle:focus-visible {
      outline: 3px solid var(--focus);
      outline-offset: 3px;
    }

    .shell {
      width: min(1360px, calc(100vw - 32px));
      margin: 24px auto;
      display: grid;
      grid-template-columns: 380px minmax(0, 1fr);
      gap: 20px;
      position: relative;
      z-index: 1;
    }

    .panel {
      position: relative;
      overflow: hidden;
      border: 1px solid var(--line);
      border-radius: var(--radius-xl);
      background: var(--surface);
      backdrop-filter: blur(18px);
      box-shadow: var(--shadow-lg);
      transition: background 220ms ease, border-color 220ms ease, box-shadow 220ms ease;
    }

    .panel::before {
      content: "";
      position: absolute;
      inset: 0;
      pointer-events: none;
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.55), transparent 28%);
    }

    html[data-theme="dark"] .panel::before {
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.06), transparent 28%);
    }

    .sidebar,
    .detail {
      position: relative;
      z-index: 1;
      min-height: calc(100vh - 48px);
    }

    .sidebar {
      padding: 24px;
      display: flex;
      flex-direction: column;
      gap: 18px;
    }

    .detail {
      padding: 28px;
    }

    .eyebrow {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      font-size: 0.75rem;
      letter-spacing: 0.18em;
      text-transform: uppercase;
      color: var(--accent-strong);
    }

    .eyebrow::before {
      content: "";
      width: 30px;
      height: 1px;
      background: currentColor;
    }

    h1, h2, h3, h4, p {
      margin: 0;
    }

    h1, h2, h3, h4 {
      line-height: 1.04;
      font-weight: 600;
      letter-spacing: -0.04em;
    }

    h1 { font-size: clamp(2.5rem, 3vw, 3.25rem); }
    h2 { font-size: clamp(1.8rem, 2.4vw, 2.45rem); }
    h3 { font-size: 1.05rem; }
    h4 { font-size: 1rem; }

    .muted,
    .meta,
    .subhead {
      color: var(--muted);
      line-height: 1.6;
      transition: color 220ms ease;
    }

    .sidebar-copy {
      font-size: 1.02rem;
      color: var(--muted);
      line-height: 1.7;
      max-width: 28ch;
    }

    .sidebar-top {
      display: grid;
      gap: 12px;
    }

    .sync-bar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 10px 12px;
      border-radius: 14px;
      border: 1px solid var(--line);
      background: rgba(255, 255, 255, 0.52);
      color: var(--muted);
      font-size: 0.84rem;
      box-shadow: var(--shadow-sm);
    }

    html[data-theme="dark"] .sync-bar {
      background: rgba(255, 255, 255, 0.05);
    }

    .sync-state {
      display: inline-flex;
      align-items: center;
      gap: 8px;
    }

    .sync-state::before {
      content: "";
      width: 8px;
      height: 8px;
      border-radius: 999px;
      background: var(--accent);
      box-shadow: 0 0 0 0 rgba(15, 118, 110, 0.24);
      animation: pulse 2.2s infinite;
    }

    .sync-state.paused::before {
      background: var(--warning);
      animation: none;
      box-shadow: none;
    }

    .sidebar-head {
      display: flex;
      justify-content: space-between;
      gap: 14px;
      align-items: start;
    }

    .theme-toggle {
      appearance: none;
      border: 1px solid var(--line);
      background: rgba(255, 255, 255, 0.46);
      color: var(--ink);
      width: 46px;
      height: 46px;
      border-radius: 999px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      box-shadow: var(--shadow-sm);
      transition: transform 160ms ease, border-color 220ms ease, background 220ms ease, color 220ms ease;
    }

    .theme-toggle:hover {
      transform: translateY(-1px) rotate(6deg);
      border-color: var(--accent);
    }

    .theme-toggle::before {
      content: var(--theme-icon);
      font-size: 1.1rem;
      line-height: 1;
    }

    html[data-theme="dark"] .theme-toggle {
      background: rgba(255, 255, 255, 0.08);
    }

    .status-banner {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 14px;
      padding: 14px 16px;
      border-radius: var(--radius-md);
      background: linear-gradient(135deg, rgba(23, 32, 42, 0.94), rgba(15, 118, 110, 0.9));
      color: white;
      box-shadow: var(--shadow-md);
      overflow: hidden;
      isolation: isolate;
    }

    .status-banner::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(120deg, transparent 10%, rgba(255, 255, 255, 0.14) 46%, transparent 70%);
      transform: translateX(-100%);
      animation: sweep 5.4s ease-in-out infinite;
      z-index: -1;
    }

    html[data-theme="dark"] .status-banner {
      background: linear-gradient(135deg, rgba(12, 25, 33, 0.96), rgba(15, 118, 110, 0.88));
    }

    .status-banner strong {
      display: block;
      font-size: 1rem;
      margin-bottom: 4px;
    }

    .pulse-dot {
      width: 12px;
      height: 12px;
      border-radius: 999px;
      background: #7cf2c2;
      box-shadow: 0 0 0 0 rgba(124, 242, 194, 0.45);
      animation: pulse 2s infinite;
      flex: none;
    }

    .sidebar-stats,
    .overview-grid,
    .detail-grid,
    .compare-grid,
    .signal-grid {
      display: grid;
      gap: 14px;
    }

    .sidebar-stats {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .list-controls {
      display: grid;
      gap: 10px;
    }

    .control-input {
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 14px;
      padding: 12px 14px;
      background: rgba(255, 255, 255, 0.78);
      color: var(--ink);
      box-shadow: var(--shadow-sm);
      transition: border-color 160ms ease, background 220ms ease, color 220ms ease;
    }

    .control-input::placeholder {
      color: var(--muted);
    }

    .control-input:focus-visible {
      outline: 3px solid var(--focus);
      outline-offset: 2px;
      border-color: var(--accent);
    }

    html[data-theme="dark"] .control-input {
      background: rgba(255, 255, 255, 0.06);
    }

    .overview-grid {
      grid-template-columns: repeat(4, minmax(0, 1fr));
      margin: 22px 0 26px;
    }

    .signal-grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      margin-bottom: 24px;
    }

    .detail-grid {
      grid-template-columns: minmax(0, 1.12fr) minmax(320px, 0.88fr);
      align-items: start;
      margin-top: 24px;
    }

    .compare-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .mini-card,
    .stat-card,
    .section-card,
    .signal-card,
    .incident-card,
    .compare-card,
    .timeline-item,
    .empty,
    .skeleton {
      border: 1px solid var(--line);
      border-radius: var(--radius-lg);
      background: var(--surface-strong);
      box-shadow: var(--shadow-sm);
      transition: background 220ms ease, border-color 220ms ease, box-shadow 220ms ease, transform 220ms ease, opacity 240ms ease;
    }

    .mini-card,
    .stat-card,
    .signal-card,
    .compare-card {
      padding: 16px;
    }

    .mini-card strong,
    .stat-card strong,
    .signal-card strong {
      display: block;
      margin-top: 10px;
      font-size: 1.65rem;
      letter-spacing: -0.05em;
    }

    .label {
      color: var(--muted);
      text-transform: uppercase;
      letter-spacing: 0.1em;
      font-size: 0.77rem;
    }

    .signal-card {
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.98), rgba(251, 247, 240, 0.96));
      position: relative;
      overflow: hidden;
    }

    .signal-card::after {
      content: "";
      position: absolute;
      left: 0;
      right: 0;
      bottom: 0;
      height: 4px;
      background: linear-gradient(90deg, var(--accent), rgba(15, 118, 110, 0.22));
    }

    .signal-card.warning::after {
      background: linear-gradient(90deg, #ea580c, rgba(234, 88, 12, 0.2));
    }

    .signal-card.danger::after {
      background: linear-gradient(90deg, #dc2626, rgba(220, 38, 38, 0.2));
    }

    html[data-theme="dark"] .signal-card {
      background: linear-gradient(180deg, rgba(24, 37, 48, 0.98), rgba(20, 31, 40, 0.96));
    }

    .incident-list {
      display: grid;
      gap: 12px;
      overflow: auto;
      padding-right: 4px;
    }

    .incident-card {
      padding: 16px;
      cursor: pointer;
      text-align: left;
      transition: transform 180ms ease, border-color 180ms ease, background 180ms ease, box-shadow 180ms ease;
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.86), rgba(252, 247, 239, 0.76));
    }

    html[data-theme="dark"] .incident-card {
      background: linear-gradient(180deg, rgba(25, 38, 49, 0.92), rgba(21, 32, 40, 0.88));
    }

    .incident-card:hover,
    .incident-card.active {
      transform: translateY(-2px);
      border-color: rgba(15, 118, 110, 0.34);
      background: linear-gradient(180deg, rgba(230, 247, 244, 0.9), rgba(248, 252, 251, 0.92));
      box-shadow: 0 18px 32px rgba(15, 118, 110, 0.12);
    }

    html[data-theme="dark"] .incident-card:hover,
    html[data-theme="dark"] .incident-card.active {
      background: linear-gradient(180deg, rgba(20, 58, 58, 0.9), rgba(18, 43, 46, 0.92));
    }

    .incident-card-head {
      display: flex;
      justify-content: space-between;
      gap: 14px;
      align-items: flex-start;
    }

    .incident-card p:last-child {
      margin-top: 10px;
    }

    .badge-row {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      align-items: center;
    }

    .badge {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      padding: 6px 11px;
      border-radius: 999px;
      font-size: 0.72rem;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      font-weight: 700;
      border: 1px solid transparent;
    }

    .badge.high { background: var(--danger-soft); color: var(--danger); }
    .badge.medium { background: var(--accent-soft); color: var(--accent-strong); }
    .badge.environment { background: rgba(23, 32, 42, 0.07); color: var(--ink); border-color: rgba(23, 32, 42, 0.06); }
    .badge.subtle { background: rgba(15, 118, 110, 0.08); color: var(--accent-strong); }
    .badge.open { background: rgba(245, 158, 11, 0.16); color: #b45309; }
    .badge.resolved { background: rgba(34, 197, 94, 0.16); color: #15803d; }

    html[data-theme="dark"] .badge.environment {
      background: rgba(255, 255, 255, 0.06);
      border-color: rgba(255, 255, 255, 0.06);
    }

    .hero {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 20px;
      align-items: start;
      padding-bottom: 22px;
      border-bottom: 1px solid var(--line);
    }

    .hero-copy {
      display: grid;
      gap: 12px;
      max-width: 800px;
    }

    .hero-actions {
      display: flex;
      flex-wrap: wrap;
      gap: 10px;
      justify-content: flex-end;
      align-items: flex-start;
    }

    .button,
    .refresh-link {
      appearance: none;
      border: 1px solid transparent;
      border-radius: 16px;
      padding: 12px 16px;
      cursor: pointer;
      font-weight: 600;
      text-decoration: none;
      transition: transform 150ms ease, border-color 150ms ease, background 150ms ease, box-shadow 150ms ease, color 220ms ease;
    }

    .button:hover,
    .refresh-link:hover {
      transform: translateY(-1px);
    }

    .button-primary {
      color: white;
      min-width: 170px;
      background: linear-gradient(135deg, var(--ink), #233242);
      box-shadow: 0 14px 24px rgba(23, 32, 42, 0.18);
    }

    html[data-theme="dark"] .button-primary {
      background: linear-gradient(135deg, #dff5f3, #78d6ca);
      color: #0f1a22;
    }

    .button-primary:disabled {
      opacity: 0.7;
      cursor: wait;
      transform: none;
    }

    .button-secondary {
      background: rgba(255, 255, 255, 0.7);
      color: var(--ink);
      border-color: var(--line);
    }

    html[data-theme="dark"] .button-secondary,
    html[data-theme="dark"] .refresh-link {
      background: rgba(255, 255, 255, 0.06);
    }

    .refresh-link {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      background: rgba(255, 255, 255, 0.7);
      color: var(--ink);
      border-color: var(--line);
    }

    .section-card {
      padding: 18px;
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.98), rgba(251, 247, 239, 0.94));
    }

    html[data-theme="dark"] .section-card,
    html[data-theme="dark"] .compare-card,
    html[data-theme="dark"] .timeline-item,
    html[data-theme="dark"] .callout,
    html[data-theme="dark"] pre,
    html[data-theme="dark"] .empty,
    html[data-theme="dark"] .mini-card,
    html[data-theme="dark"] .stat-card {
      background: linear-gradient(180deg, rgba(24, 37, 48, 0.98), rgba(20, 31, 40, 0.94));
    }

    .section-card + .section-card {
      margin-top: 16px;
    }

    .section-heading {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      align-items: baseline;
      margin-bottom: 14px;
    }

    .section-heading h3 {
      color: var(--muted);
      text-transform: uppercase;
      letter-spacing: 0.12em;
      font-size: 0.82rem;
    }

    .compare-card {
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.94), rgba(250, 246, 238, 0.92));
    }

    .compare-card h4 {
      margin-bottom: 14px;
    }

    .compare-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      align-items: baseline;
      padding: 10px 0;
      border-top: 1px solid rgba(180, 158, 129, 0.18);
    }

    .compare-row:first-of-type {
      border-top: none;
      padding-top: 0;
    }

    .compare-row strong {
      font-size: 1.15rem;
    }

    .callout,
    pre {
      margin: 0;
      border-radius: var(--radius-md);
      border: 1px solid rgba(180, 158, 129, 0.2);
      background: rgba(251, 248, 241, 0.96);
      padding: 16px;
      line-height: 1.65;
      white-space: pre-wrap;
      word-break: break-word;
      overflow-wrap: anywhere;
      font-size: 0.98rem;
    }

    .signature-chip {
      display: flex;
      flex-wrap: wrap;
      gap: 10px;
      align-items: center;
      padding: 12px 14px;
      border-radius: var(--radius-md);
      background: var(--warning-soft);
      color: var(--warning);
      border: 1px solid rgba(234, 88, 12, 0.16);
      line-height: 1.55;
    }

    .notes-box {
      width: 100%;
      min-height: 120px;
      border: 1px solid var(--line);
      border-radius: 16px;
      padding: 14px;
      background: rgba(255, 255, 255, 0.72);
      color: var(--ink);
      resize: vertical;
      box-shadow: var(--shadow-sm);
    }

    html[data-theme="dark"] .notes-box {
      background: rgba(255, 255, 255, 0.05);
    }

    .notes-actions {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      align-items: center;
      margin-top: 12px;
    }

    .timeline {
      display: grid;
      gap: 14px;
    }

    .timeline-item {
      display: grid;
      grid-template-columns: 160px 1fr;
      gap: 16px;
      align-items: start;
      padding: 16px;
      position: relative;
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.95), rgba(252, 247, 239, 0.86));
    }

    .timeline-item::before {
      content: "";
      position: absolute;
      left: 14px;
      top: 16px;
      bottom: 16px;
      width: 4px;
      border-radius: 999px;
      background: linear-gradient(180deg, var(--accent), rgba(15, 118, 110, 0.08));
    }

    .timeline-time {
      color: var(--muted);
      font-size: 0.9rem;
      padding-left: 18px;
    }

    .timeline-content {
      padding-left: 10px;
    }

    .timeline-content strong {
      display: block;
      margin-bottom: 6px;
      font-size: 1rem;
    }

    .empty {
      padding: 26px;
      color: var(--muted);
      border-style: dashed;
      background: rgba(255, 255, 255, 0.6);
    }

    .loading-state {
      display: grid;
      gap: 18px;
    }

    .skeleton {
      position: relative;
      overflow: hidden;
      background: rgba(255, 255, 255, 0.68);
      min-height: 96px;
    }

    .skeleton::after {
      content: "";
      position: absolute;
      inset: 0;
      transform: translateX(-100%);
      background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.75), transparent);
      animation: shimmer 1.5s infinite;
    }

    html[data-theme="dark"] .skeleton {
      background: rgba(255, 255, 255, 0.04);
    }

    html[data-theme="dark"] .skeleton::after {
      background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.08), transparent);
    }

    .skeleton.hero { min-height: 180px; }
    .skeleton.row { min-height: 82px; }
    .skeleton.panel-block { min-height: 210px; }

    .reveal {
      opacity: 0;
      transform: translateY(24px) scale(0.985);
      transition: opacity 560ms ease, transform 560ms cubic-bezier(.2,.7,.2,1);
      will-change: transform, opacity;
    }

    .reveal.in-view {
      opacity: 1;
      transform: translateY(0) scale(1);
    }

    .reveal-delay-1 { transition-delay: 80ms; }
    .reveal-delay-2 { transition-delay: 140ms; }
    .reveal-delay-3 { transition-delay: 200ms; }

    .float-accent {
      position: absolute;
      width: 240px;
      height: 240px;
      border-radius: 999px;
      background: radial-gradient(circle, var(--accent-glow) 0%, transparent 68%);
      filter: blur(10px);
      pointer-events: none;
      z-index: 0;
      animation: drift 12s ease-in-out infinite;
    }

    .float-accent.one { top: -60px; right: 8%; }
    .float-accent.two { bottom: 18%; left: -60px; animation-delay: -4s; }

    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      padding: 0;
      margin: -1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
      white-space: nowrap;
      border: 0;
    }

    @keyframes shimmer {
      100% { transform: translateX(100%); }
    }

    @keyframes pulse {
      0% { box-shadow: 0 0 0 0 rgba(124, 242, 194, 0.45); }
      70% { box-shadow: 0 0 0 12px rgba(124, 242, 194, 0); }
      100% { box-shadow: 0 0 0 0 rgba(124, 242, 194, 0); }
    }

    @keyframes drift {
      0%, 100% { transform: translate3d(0, 0, 0) scale(1); }
      50% { transform: translate3d(0, -12px, 0) scale(1.04); }
    }

    @keyframes sweep {
      0%, 100% { transform: translateX(-100%); }
      50% { transform: translateX(100%); }
    }

    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after {
        animation: none !important;
        transition: none !important;
        scroll-behavior: auto !important;
      }

      .reveal {
        opacity: 1 !important;
        transform: none !important;
      }
    }

    @media (max-width: 1120px) {
      .shell { grid-template-columns: 1fr; }
      .sidebar, .detail { min-height: auto; }
      .overview-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      .signal-grid { grid-template-columns: 1fr; }
      .detail-grid { grid-template-columns: 1fr; }
    }

    @media (max-width: 760px) {
      .shell {
        width: min(100vw - 18px, 100%);
        margin: 10px auto 18px;
      }

      .panel { border-radius: 22px; }
      .sidebar, .detail { padding: 18px; }
      .hero { grid-template-columns: 1fr; }
      .hero-actions { justify-content: stretch; }
      .button-primary, .button-secondary, .refresh-link { width: 100%; }
      .overview-grid, .sidebar-stats, .compare-grid { grid-template-columns: 1fr; }
      .timeline-item { grid-template-columns: 1fr; }
      .timeline-time, .timeline-content { padding-left: 18px; }
      .sidebar-head { align-items: center; }
    }
  </style>
</head>
<body>
  <div class="float-accent one" aria-hidden="true"></div>
  <div class="float-accent two" aria-hidden="true"></div>
  <div class="shell">
    <aside class="panel sidebar reveal" aria-label="Incident list">
      <div class="sidebar-top">
        <div class="sidebar-head">
          <div>
            <div class="eyebrow">Release safety</div>
            <h1>watchdog</h1>
          </div>
          <button id="theme-toggle" class="theme-toggle" type="button" aria-label="Toggle dark mode" title="Toggle dark mode"></button>
        </div>
        <p class="sidebar-copy">A Rust incident console that turns deploy regressions into readable evidence. It correlates releases, metric shifts, and new log signatures so developers can understand what changed fast.</p>
      </div>

      <section class="sync-bar reveal reveal-delay-1" aria-label="Dashboard sync status">
        <div id="sync-state" class="sync-state">Auto-refresh on</div>
        <div id="sync-time">Waiting for first sync</div>
      </section>

      <section class="status-banner reveal reveal-delay-1" aria-label="Monitoring status">
        <div>
          <strong>Monitoring deploy risk</strong>
          <div class="meta" style="color: rgba(255,255,255,0.82);">Live incident view for deployment-linked regressions</div>
        </div>
        <span class="pulse-dot" aria-hidden="true"></span>
      </section>

      <section class="sidebar-stats reveal reveal-delay-2" aria-label="Dashboard summary">
        <article class="mini-card">
          <div class="label">Incidents</div>
          <strong id="incident-count">0</strong>
        </article>
        <article class="mini-card">
          <div class="label">High Severity</div>
          <strong id="high-count">0</strong>
        </article>
        <article class="mini-card">
          <div class="label">AI Cached</div>
          <strong id="cached-count">0</strong>
        </article>
      </section>

      <section class="list-controls reveal reveal-delay-3" aria-label="Incident filters">
        <input id="incident-search" class="control-input" type="search" placeholder="Search deploys, summaries, environments" aria-label="Search incidents" />
        <select id="severity-filter" class="control-input" aria-label="Filter incidents by severity">
          <option value="all">All severities</option>
          <option value="high">High severity</option>
          <option value="medium">Medium severity</option>
          <option value="cached">Has AI explanation</option>
        </select>
      </section>

      <div id="incident-list" class="incident-list reveal reveal-delay-3" role="list" aria-label="Incident list"></div>
    </aside>

    <main class="panel detail" id="detail-panel" aria-live="polite"></main>
  </div>

  <script>
    let incidents = [];
    let activeIncidentId = null;
    let searchQuery = '';
    let severityFilter = 'all';
    let lastSyncedAt = null;
    let refreshTimer = null;
    let isExplainingIncident = false;
    const THEME_KEY = 'watchdog-theme';
    const REFRESH_INTERVAL_MS = 5000;
    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    document.addEventListener('DOMContentLoaded', () => {
      applySavedTheme();
      bindThemeToggle();
      bindFilters();
      bindVisibilityRefresh();
      renderEmptyDetail();
      setupRevealObserver();
      loadIncidents();
      startAutoRefresh();
    });

    async function loadIncidents(options = {}) {
      const { silent = false } = options;
      if (!silent) {
        renderIncidentListLoading();
      }
      try {
        const response = await fetch('/api/incidents');
        if (!response.ok) {
          throw new Error(await response.text());
        }
        incidents = await response.json();
        lastSyncedAt = new Date();
        renderSidebarStats();
        renderIncidentList();
        renderSyncStatus();

        if (incidents.length) {
          const nextId = activeIncidentId && incidents.some((incident) => incident.id === activeIncidentId)
            ? activeIncidentId
            : incidents[0].id;
          if (!isExplainingIncident) {
            await selectIncident(nextId, false, silent);
          }
        } else if (!silent) {
          renderEmptyDetail();
        }
      } catch (error) {
        renderSyncStatus(error.message || String(error));
        if (!silent) {
          renderSidebarStats();
          renderIncidentListError(error);
          renderErrorDetail(error);
        }
      }
    }

    function applySavedTheme() {
      const savedTheme = localStorage.getItem(THEME_KEY);
      const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      const theme = savedTheme || (systemDark ? 'dark' : 'light');
      document.documentElement.setAttribute('data-theme', theme);
    }

    function bindThemeToggle() {
      const toggle = document.getElementById('theme-toggle');
      updateThemeToggleLabel(toggle);
      toggle.addEventListener('click', () => {
        const current = document.documentElement.getAttribute('data-theme') || 'light';
        const next = current === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        localStorage.setItem(THEME_KEY, next);
        updateThemeToggleLabel(toggle);
      });
    }

    function updateThemeToggleLabel(toggle) {
      const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
      const label = isDark ? 'Switch to light mode' : 'Switch to dark mode';
      toggle.setAttribute('aria-label', label);
      toggle.setAttribute('title', label);
    }


    function bindVisibilityRefresh() {
      document.addEventListener('visibilitychange', () => {
        if (!document.hidden) {
          loadIncidents({ silent: true });
        }
      });
    }

    function startAutoRefresh() {
      if (refreshTimer) {
        clearInterval(refreshTimer);
      }

      refreshTimer = setInterval(() => {
        if (!document.hidden) {
          loadIncidents({ silent: true });
        }
      }, REFRESH_INTERVAL_MS);
    }

    function renderSyncStatus(errorMessage = null) {
      const state = document.getElementById('sync-state');
      const time = document.getElementById('sync-time');
      if (!state || !time) {
        return;
      }

      if (errorMessage) {
        state.textContent = 'Auto-refresh paused';
        state.classList.add('paused');
        time.textContent = errorMessage;
        return;
      }

      state.textContent = 'Auto-refresh on';
      state.classList.remove('paused');
      time.textContent = lastSyncedAt
        ? `Last synced ${lastSyncedAt.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' })}`
        : 'Waiting for first sync';
    }

    function setupRevealObserver() {
      if (prefersReducedMotion || !('IntersectionObserver' in window)) {
        document.querySelectorAll('.reveal').forEach((element) => element.classList.add('in-view'));
        return;
      }

      const observer = new IntersectionObserver((entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add('in-view');
            observer.unobserve(entry.target);
          }
        });
      }, { threshold: 0.12, rootMargin: '0px 0px -6% 0px' });

      document.querySelectorAll('.reveal').forEach((element) => observer.observe(element));
    }

    function activateReveals(scope = document) {
      const elements = scope.querySelectorAll('.reveal');
      if (prefersReducedMotion) {
        elements.forEach((element) => element.classList.add('in-view'));
        return;
      }

      elements.forEach((element, index) => {
        requestAnimationFrame(() => {
          setTimeout(() => element.classList.add('in-view'), Math.min(index * 40, 180));
        });
      });
    }

    function bindFilters() {
      document.getElementById('incident-search').addEventListener('input', (event) => {
        searchQuery = event.target.value.trim().toLowerCase();
        renderIncidentList();
      });

      document.getElementById('severity-filter').addEventListener('change', (event) => {
        severityFilter = event.target.value;
        renderIncidentList();
      });
    }

    function visibleIncidents() {
      return incidents.filter((incident) => {
        const haystack = [incident.deploy_id, incident.summary, incident.environment].join(' ').toLowerCase();
        const matchesSearch = !searchQuery || haystack.includes(searchQuery);
        const matchesSeverity = severityFilter === 'all'
          || (severityFilter === 'cached' ? incident.has_cached_explanation : incident.severity === severityFilter);
        return matchesSearch && matchesSeverity;
      });
    }

    function renderSidebarStats() {
      document.getElementById('incident-count').textContent = incidents.length;
      document.getElementById('high-count').textContent = incidents.filter((incident) => incident.severity === 'high').length;
      document.getElementById('cached-count').textContent = incidents.filter((incident) => incident.has_cached_explanation).length;
    }

    function renderIncidentListLoading() {
      const container = document.getElementById('incident-list');
      container.innerHTML = Array.from({ length: 3 }).map((_, index) => `<div class="skeleton row reveal reveal-delay-${Math.min(index + 1, 3)}" aria-hidden="true"></div>`).join('');
      activateReveals(container);
    }

    function renderIncidentListError(error) {
      const container = document.getElementById('incident-list');
      container.innerHTML = `<div class="empty reveal in-view">Unable to load incidents. ${escapeHtml(error.message || String(error))}</div>`;
    }

    function renderIncidentList() {
      const container = document.getElementById('incident-list');
      const visible = visibleIncidents();
      if (!visible.length) {
        container.innerHTML = '<div class="empty reveal in-view">No incidents match the current filters. Try clearing the search or changing the severity filter.</div>';
        return;
      }

      container.innerHTML = visible.map((incident, index) => renderIncidentCard(incident, index)).join('');
      activateReveals(container);
    }

    function renderIncidentCard(incident, index) {
      const isActive = incident.id === activeIncidentId;
      const delayClass = `reveal-delay-${Math.min((index % 3) + 1, 3)}`;
      return `
        <article
          class="incident-card reveal ${delayClass} ${isActive ? 'active in-view' : ''}"
          onclick="selectIncident('${incident.id}')"
          onkeydown="handleIncidentKey(event, '${incident.id}')"
          tabindex="0"
          role="button"
          aria-pressed="${isActive}"
          aria-label="Open incident ${escapeHtml(incident.deploy_id)}"
        >
          <div class="incident-card-head">
            <div class="badge-row">
              ${renderBadge(incident.severity, incident.severity)}
              ${renderBadge(incident.status, incident.status)}
              ${renderBadge('environment', incident.environment)}
              ${incident.has_cached_explanation ? renderBadge('subtle', 'AI cached') : ''}
              ${incident.has_notes ? renderBadge('subtle', 'Notes') : ''}
            </div>
            ${renderBadge('subtle', new Date(incident.created_at).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }))}
          </div>
          <h3 style="margin-top: 10px;">${escapeHtml(incident.deploy_id)}</h3>
          <p class="meta">${new Date(incident.created_at).toLocaleString()}</p>
          <p class="meta">${escapeHtml(incident.summary)}</p>
        </article>
      `;
    }

    function handleIncidentKey(event, id) {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        selectIncident(id);
      }
    }

    async function selectIncident(id, showLoading = true, preserveCurrentDetail = false) {
      activeIncidentId = id;
      renderIncidentList();
      if (showLoading) {
        renderDetailLoading();
      } else if (!preserveCurrentDetail && !isExplainingIncident) {
        renderDetailLoading();
      }

      try {
        const response = await fetch(`/api/incidents/${id}`);
        if (!response.ok) {
          throw new Error(await response.text());
        }
        const incident = await response.json();
        renderDetail(incident, null, false, null);
      } catch (error) {
        renderErrorDetail(error);
      }
    }

    function renderEmptyDetail() {
      document.getElementById('detail-panel').innerHTML = `
        <div class="empty reveal in-view">
          No incidents yet. Run the daemon and trigger a bad deploy simulation to populate the dashboard.
        </div>
      `;
    }

    function renderDetailLoading() {
      document.getElementById('detail-panel').innerHTML = `
        <div class="loading-state" aria-label="Loading incident details">
          <div class="skeleton hero reveal in-view"></div>
          <div class="overview-grid">
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
          </div>
          <div class="detail-grid">
            <div class="skeleton panel-block reveal in-view"></div>
            <div class="skeleton panel-block reveal in-view"></div>
          </div>
        </div>
      `;
    }

    function renderErrorDetail(error) {
      document.getElementById('detail-panel').innerHTML = `
        <div class="empty reveal in-view">
          Could not load this incident. ${escapeHtml(error.message || String(error))}
        </div>
      `;
    }

    function renderDetail(incident, explanation, loading, explanationError) {
      const panel = document.getElementById('detail-panel');
      const verdict = incident.verdict;
      const comparison = verdict.comparison;
      const signature = verdict.top_error_signature || 'No dominant error signature captured';
      const resolvedExplanation = explanation || incident.cached_explanation;
      const explanationBody = resolvedExplanation
        ? escapeHtml(resolvedExplanation)
        : explanationError
          ? `Explanation failed: ${escapeHtml(explanationError)}`
          : 'No explanation generated yet. Click "Explain Incident" to get an evidence-grounded summary and debugging steps.';
      const explanationMeta = incident.cached_explanation_updated_at
        ? `Cached ${new Date(incident.cached_explanation_updated_at).toLocaleString()}`
        : 'Generated on demand';

      panel.innerHTML = `
        <section class="hero reveal in-view">
          <div class="hero-copy">
            <div class="badge-row">
              ${renderBadge(incident.severity, incident.severity)}
              ${renderBadge(incident.status, incident.status)}
              ${renderBadge('environment', verdict.environment)}
              ${renderBadge('subtle', `deploy ${escapeHtml(verdict.deploy_id)}`)}
            </div>
            <h2>${escapeHtml(incident.summary)}</h2>
            <p class="subhead">Watchdog flagged this release ${verdict.seconds_after_deploy}s after deploy. The detector saw a statistically meaningful shift in service health and preserved the strongest supporting evidence.</p>
          </div>
          <div class="hero-actions">
            <button class="button button-primary" ${loading ? 'disabled' : ''} onclick="explainIncident('${incident.id}')">${loading ? 'Explaining…' : 'Explain Incident'}</button>
            <button class="button button-secondary" onclick="loadIncidents()">Refresh Incidents</button>
            <a class="refresh-link" href="/api/incidents/${incident.id}/export/markdown">Download Markdown</a>
            <a class="refresh-link" href="/api/incidents/${incident.id}/export/json">Download JSON</a>
          </div>
        </section>

        <section class="signal-grid" aria-label="Primary signals">
          ${renderSignalCard('Detection Delay', `${verdict.seconds_after_deploy}s`, 'Time between deploy and detected regression', 'warning', 'reveal reveal-delay-1')}
          ${renderSignalCard('Top Error Signature', signature, verdict.top_error_count ? `Seen ${verdict.top_error_count} times after deploy` : 'No repeated new error count captured', 'danger', 'reveal reveal-delay-2')}
          ${renderSignalCard('Requests at Detection', `${comparison.request_rate_at_detection.toFixed(1)} req/s`, 'Traffic volume when the verdict was raised', '', 'reveal reveal-delay-3')}
        </section>

        <section class="overview-grid" aria-label="Incident overview">
          ${renderStatCard('Error Rate Delta', verdict.error_rate_delta.toFixed(3), 'reveal reveal-delay-1')}
          ${renderStatCard('Latency Delta', `${verdict.latency_delta_ms.toFixed(1)} ms`, 'reveal reveal-delay-2')}
          ${renderStatCard('Deploy Time', formatDateTime(verdict.deploy_timestamp), 'reveal reveal-delay-3')}
          ${renderStatCard('Detected At', formatDateTime(verdict.detected_at), 'reveal reveal-delay-3')}
        </section>

        <section class="detail-grid">
          <div>
            <article class="section-card reveal reveal-delay-1">
              <div class="section-heading">
                <h3>Before vs After</h3>
                <span class="muted">Baseline against detected state</span>
              </div>
              <div class="compare-grid">
                ${renderCompareCard('Error Rate', comparison.baseline_error_rate.toFixed(3), comparison.detected_error_rate.toFixed(3))}
                ${renderCompareCard('P95 Latency', `${comparison.baseline_latency_ms.toFixed(1)} ms`, `${comparison.detected_latency_ms.toFixed(1)} ms`)}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-2">
              <div class="section-heading">
                <h3>Incident Timeline</h3>
                <span class="muted">What happened, in order</span>
              </div>
              <div class="timeline">${verdict.timeline.map((event) => renderTimelineItem(event)).join('')}</div>
            </article>
          </div>

          <div>
            <article class="section-card reveal reveal-delay-1">
              <div class="section-heading">
                <h3>Why Watchdog Flagged It</h3>
                <span class="muted">Detector verdict</span>
              </div>
              <div class="callout">${escapeHtml(incident.alert_text)}</div>
            </article>

            <article class="section-card reveal reveal-delay-2">
              <div class="section-heading">
                <h3>Dominant Error Signature</h3>
                <span class="muted">Most repeated new error</span>
              </div>
              <div class="signature-chip">
                <strong>${escapeHtml(signature)}</strong>
                ${verdict.top_error_count ? `<span>Seen ${verdict.top_error_count} times after deploy</span>` : '<span>No repeated count available</span>'}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>Incident Workflow</h3>
                <span class="muted">Track the investigation state</span>
              </div>
              <div class="badge-row">
                ${renderBadge(incident.status, incident.status)}
                ${incident.notes.trim() ? renderBadge('subtle', 'Notes saved') : renderBadge('subtle', 'No notes yet')}
              </div>
              <div class="hero-actions" style="margin-top: 14px; justify-content: flex-start;">
                <button class="button button-secondary" ${incident.status === 'open' ? 'disabled' : ''} onclick="setIncidentStatus('${incident.id}', 'open')">Mark Open</button>
                <button class="button button-secondary" ${incident.status === 'resolved' ? 'disabled' : ''} onclick="setIncidentStatus('${incident.id}', 'resolved')">Mark Resolved</button>
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>Investigation Notes</h3>
                <span class="muted">Persisted with the incident</span>
              </div>
              <textarea id="incident-notes" class="notes-box" placeholder="Capture what you found, what changed, and what to check next...">${escapeHtml(incident.notes)}</textarea>
              <div class="notes-actions">
                <span class="muted">Notes are saved back into the incident file</span>
                <button class="button button-secondary" onclick="saveIncidentNotes('${incident.id}')">Save Notes</button>
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>AI Explanation</h3>
                <span class="muted">${escapeHtml(explanationMeta)}</span>
              </div>
              <pre>${explanationBody}</pre>
            </article>
          </div>
        </section>
      `;

      activateReveals(panel);
    }


    async function setIncidentStatus(id, status) {
      const response = await fetch(`/api/incidents/${id}/status`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status }),
      });

      if (!response.ok) {
        throw new Error(await response.text());
      }

      const incident = await response.json();
      await loadIncidents({ silent: true });
      renderDetail(incident, incident.cached_explanation, false, null);
    }

    async function saveIncidentNotes(id) {
      const notes = document.getElementById('incident-notes')?.value || '';
      const response = await fetch(`/api/incidents/${id}/notes`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ notes }),
      });

      if (!response.ok) {
        throw new Error(await response.text());
      }

      const incident = await response.json();
      await loadIncidents({ silent: true });
      renderDetail(incident, incident.cached_explanation, false, null);
    }

    async function explainIncident(id) {
      let incident;
      isExplainingIncident = true;
      try {
        const incidentResponse = await fetch(`/api/incidents/${id}`);
        if (!incidentResponse.ok) {
          throw new Error(await incidentResponse.text());
        }
        incident = await incidentResponse.json();
        renderDetail(incident, null, true, null);

        const explainResponse = await fetch(`/api/incidents/${id}/explain`, { method: 'POST' });
        const body = await explainResponse.text();
        renderDetail(
          incident,
          explainResponse.ok ? JSON.parse(body).explanation : null,
          false,
          explainResponse.ok ? null : body,
        );
      } catch (error) {
        if (incident) {
          renderDetail(incident, null, false, error.message || String(error));
        } else {
          renderErrorDetail(error);
        }
      } finally {
        isExplainingIncident = false;
      }
    }

    function renderBadge(kind, label) {
      return `<span class="badge ${kind}">${escapeHtml(label)}</span>`;
    }

    function renderStatCard(label, value, extraClass = '') {
      return `
        <article class="stat-card ${extraClass}">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
        </article>
      `;
    }

    function renderSignalCard(label, value, detail, tone, extraClass = '') {
      return `
        <article class="signal-card ${tone} ${extraClass}">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
          <p class="meta" style="margin-top: 8px;">${escapeHtml(detail)}</p>
        </article>
      `;
    }

    function renderCompareCard(title, baseline, detected) {
      return `
        <div class="compare-card reveal reveal-delay-1">
          <h4>${escapeHtml(title)}</h4>
          <div class="compare-row">
            <span class="row-label muted">Baseline</span>
            <strong>${escapeHtml(baseline)}</strong>
          </div>
          <div class="compare-row">
            <span class="row-label muted">Detected</span>
            <strong>${escapeHtml(detected)}</strong>
          </div>
        </div>
      `;
    }

    function renderTimelineItem(event) {
      return `
        <article class="timeline-item reveal reveal-delay-1">
          <div class="timeline-time">${new Date(event.timestamp).toLocaleTimeString()}</div>
          <div class="timeline-content">
            <strong>${escapeHtml(event.label)}</strong>
            <div class="meta">${escapeHtml(event.detail)}</div>
          </div>
        </article>
      `;
    }

    function formatDateTime(value) {
      return new Date(value).toLocaleString([], {
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      });
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }
  </script>
</body>
</html>
"#;
