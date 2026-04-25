use crate::llm;
use crate::storage;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
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

pub async fn serve(state_dir: PathBuf, host: String, port: u16) -> anyhow::Result<()> {
    let app_state = AppState {
        state_dir: Arc::new(state_dir),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/incidents", get(list_incidents))
        .route("/api/incidents/{id}", get(get_incident))
        .route("/api/incidents/{id}/explain", post(explain_incident))
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
        Ok(incidents) => Json(incidents.into_iter().map(|incident| incident.list_item()).collect::<Vec<_>>()).into_response(),
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

async fn explain_incident(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let incident = match storage::read_incident(&state.state_dir, &id) {
        Ok(Some(incident)) => incident,
        Ok(None) => return (StatusCode::NOT_FOUND, "incident not found").into_response(),
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    };

    match llm::explain_incident(&incident).await {
        Ok(explanation) => Json(ExplainResponse { explanation }).into_response(),
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
      --bg: #f7f1e8;
      --panel: #fffaf1;
      --ink: #1f2430;
      --muted: #6d7280;
      --line: #d9d1c5;
      --accent: #0f766e;
      --accent-soft: #d7f3ef;
      --danger: #a11d33;
      --danger-soft: #fde7eb;
      --shadow: 0 18px 40px rgba(31, 36, 48, 0.08);
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: Georgia, "Times New Roman", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(15, 118, 110, 0.12), transparent 35%),
        linear-gradient(180deg, #f9f4ec 0%, var(--bg) 100%);
      min-height: 100vh;
    }
    .shell {
      width: min(1220px, calc(100vw - 32px));
      margin: 32px auto;
      display: grid;
      grid-template-columns: 340px 1fr;
      gap: 20px;
    }
    .panel {
      background: rgba(255, 250, 241, 0.86);
      border: 1px solid rgba(217, 209, 197, 0.9);
      border-radius: 24px;
      box-shadow: var(--shadow);
      backdrop-filter: blur(14px);
    }
    .sidebar { padding: 24px; }
    .detail { padding: 28px; min-height: 78vh; }
    h1, h2, h3 { margin: 0; font-weight: 600; }
    h1 { font-size: 2.4rem; letter-spacing: -0.04em; }
    .subhead { margin-top: 10px; color: var(--muted); line-height: 1.5; font-size: 0.98rem; }
    .incident-list { margin-top: 22px; display: grid; gap: 12px; }
    .incident-card {
      border: 1px solid var(--line);
      border-radius: 18px;
      padding: 14px 16px;
      background: rgba(255,255,255,0.72);
      cursor: pointer;
      transition: transform 180ms ease, border-color 180ms ease, background 180ms ease;
    }
    .incident-card:hover, .incident-card.active {
      transform: translateY(-2px);
      border-color: var(--accent);
      background: rgba(215, 243, 239, 0.72);
    }
    .badge {
      display: inline-flex;
      align-items: center;
      padding: 4px 10px;
      border-radius: 999px;
      font-size: 0.72rem;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      margin-bottom: 10px;
    }
    .badge.high { background: var(--danger-soft); color: var(--danger); }
    .badge.medium { background: var(--accent-soft); color: var(--accent); }
    .meta { color: var(--muted); font-size: 0.88rem; margin-top: 8px; }
    .hero {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      align-items: start;
      border-bottom: 1px solid var(--line);
      padding-bottom: 18px;
    }
    .hero button {
      border: none;
      border-radius: 14px;
      background: var(--ink);
      color: white;
      font: inherit;
      padding: 12px 16px;
      cursor: pointer;
    }
    .hero button:disabled { opacity: 0.55; cursor: wait; }
    .metrics {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 12px;
      margin: 18px 0 22px;
    }
    .metric, .compare-card {
      border: 1px solid var(--line);
      border-radius: 18px;
      padding: 14px;
      background: rgba(255,255,255,0.66);
    }
    .metric strong, .compare-card strong {
      display: block;
      font-size: 1.35rem;
      margin-top: 8px;
    }
    .compare-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
    }
    .compare-values {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      margin-top: 12px;
      color: var(--muted);
    }
    .section { margin-top: 24px; }
    .section h3 {
      font-size: 1rem;
      margin-bottom: 10px;
      color: var(--muted);
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    .callout, pre {
      margin: 0;
      border-radius: 18px;
      border: 1px solid var(--line);
      background: rgba(255,255,255,0.72);
      padding: 16px;
      line-height: 1.55;
      white-space: pre-wrap;
      word-break: break-word;
      overflow-wrap: anywhere;
    }
    .timeline {
      display: grid;
      gap: 12px;
    }
    .timeline-item {
      display: grid;
      grid-template-columns: 150px 1fr;
      gap: 14px;
      align-items: start;
      border-left: 3px solid var(--accent);
      padding: 10px 0 10px 14px;
      background: rgba(255,255,255,0.5);
      border-radius: 0 16px 16px 0;
    }
    .timeline-time {
      color: var(--muted);
      font-size: 0.9rem;
    }
    .empty {
      color: var(--muted);
      padding: 18px;
      border: 1px dashed var(--line);
      border-radius: 18px;
      margin-top: 20px;
    }
    @media (max-width: 960px) {
      .shell { grid-template-columns: 1fr; }
      .detail { min-height: auto; }
      .metrics, .compare-grid { grid-template-columns: 1fr; }
      .timeline-item { grid-template-columns: 1fr; }
    }
  </style>
</head>
<body>
  <div class="shell">
    <aside class="panel sidebar">
      <h1>watchdog</h1>
      <p class="subhead">A local incident dashboard for deployment regressions. Detect in Rust, explain with an evidence-grounded LLM.</p>
      <div id="incident-list" class="incident-list"></div>
    </aside>
    <main class="panel detail" id="detail-panel">
      <div class="empty">No incidents yet. Run the daemon and trigger a bad deploy simulation to populate the dashboard.</div>
    </main>
  </div>
  <script>
    let incidents = [];
    let activeIncidentId = null;

    async function loadIncidents() {
      const response = await fetch('/api/incidents');
      incidents = await response.json();
      renderIncidentList();
      if (incidents.length && !activeIncidentId) {
        selectIncident(incidents[0].id);
      }
    }

    function renderIncidentList() {
      const container = document.getElementById('incident-list');
      if (!incidents.length) {
        container.innerHTML = '<div class="empty">No incidents captured yet.</div>';
        return;
      }
      container.innerHTML = incidents.map((incident) => `
        <article class="incident-card ${incident.id === activeIncidentId ? 'active' : ''}" onclick="selectIncident('${incident.id}')">
          <span class="badge ${incident.severity}">${incident.severity}</span>
          <h2>${escapeHtml(incident.deploy_id)}</h2>
          <p class="meta">${escapeHtml(incident.environment)} • ${new Date(incident.created_at).toLocaleString()}</p>
          <p class="meta">${escapeHtml(incident.summary)}</p>
        </article>
      `).join('');
    }

    async function selectIncident(id) {
      activeIncidentId = id;
      renderIncidentList();
      const response = await fetch(`/api/incidents/${id}`);
      const incident = await response.json();
      renderDetail(incident, null, false);
    }

    function renderDetail(incident, explanation, loading) {
      const panel = document.getElementById('detail-panel');
      const signature = incident.verdict.top_error_signature || 'No dominant error signature captured';
      const cmp = incident.verdict.comparison;
      const timeline = incident.verdict.timeline.map((event) => `
        <article class="timeline-item">
          <div class="timeline-time">${new Date(event.timestamp).toLocaleTimeString()}</div>
          <div>
            <strong>${escapeHtml(event.label)}</strong>
            <div class="meta">${escapeHtml(event.detail)}</div>
          </div>
        </article>
      `).join('');

      panel.innerHTML = `
        <section class="hero">
          <div>
            <span class="badge ${incident.severity}">${incident.severity}</span>
            <h2>${escapeHtml(incident.summary)}</h2>
            <p class="subhead">Deploy ${escapeHtml(incident.verdict.deploy_id)} in ${escapeHtml(incident.verdict.environment)} was flagged ${incident.verdict.seconds_after_deploy}s after release.</p>
          </div>
          <button ${loading ? 'disabled' : ''} onclick="explainIncident('${incident.id}')">${loading ? 'Explaining…' : 'Explain Incident'}</button>
        </section>
        <section class="metrics">
          <article class="metric"><div>Error Rate Delta</div><strong>${incident.verdict.error_rate_delta.toFixed(3)}</strong></article>
          <article class="metric"><div>Latency Delta</div><strong>${incident.verdict.latency_delta_ms.toFixed(1)} ms</strong></article>
          <article class="metric"><div>Detected</div><strong>${new Date(incident.verdict.detected_at).toLocaleTimeString()}</strong></article>
        </section>
        <section class="section">
          <h3>Before vs After</h3>
          <div class="compare-grid">
            <article class="compare-card">
              <div>Error Rate</div>
              <div class="compare-values"><span>Baseline</span><strong>${cmp.baseline_error_rate.toFixed(3)}</strong></div>
              <div class="compare-values"><span>Detected</span><strong>${cmp.detected_error_rate.toFixed(3)}</strong></div>
            </article>
            <article class="compare-card">
              <div>P95 Latency</div>
              <div class="compare-values"><span>Baseline</span><strong>${cmp.baseline_latency_ms.toFixed(1)} ms</strong></div>
              <div class="compare-values"><span>Detected</span><strong>${cmp.detected_latency_ms.toFixed(1)} ms</strong></div>
            </article>
          </div>
          <div class="meta" style="margin-top: 10px;">Request rate at detection: ${cmp.request_rate_at_detection.toFixed(1)} req/s</div>
        </section>
        <section class="section">
          <h3>Incident Timeline</h3>
          <div class="timeline">${timeline}</div>
        </section>
        <section class="section">
          <h3>Why Watchdog Flagged It</h3>
          <div class="callout">${escapeHtml(incident.alert_text)}</div>
        </section>
        <section class="section">
          <h3>Dominant Error Signature</h3>
          <div class="callout">${escapeHtml(signature)}${incident.verdict.top_error_count ? `\nSeen ${incident.verdict.top_error_count} times after deploy` : ''}</div>
        </section>
        <section class="section">
          <h3>AI Explanation</h3>
          <pre>${explanation ? escapeHtml(explanation) : 'No explanation generated yet. Click "Explain Incident" to get an evidence-grounded summary and debugging steps.'}</pre>
        </section>
      `;
    }

    async function explainIncident(id) {
      const response = await fetch(`/api/incidents/${id}`);
      const incident = await response.json();
      renderDetail(incident, null, true);

      const explainResponse = await fetch(`/api/incidents/${id}/explain`, { method: 'POST' });
      const text = await explainResponse.text();
      renderDetail(incident, explainResponse.ok ? JSON.parse(text).explanation : text, false);
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }

    loadIncidents();
  </script>
</body>
</html>
"#;
