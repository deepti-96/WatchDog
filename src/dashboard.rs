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

async fn explain_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
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
    }

    * { box-sizing: border-box; }
    html { color-scheme: light; }
    

    button, article, section, input {
      font: inherit;
    }

    button:focus-visible,
    article:focus-visible,
    .refresh-link:focus-visible {
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

    .status-banner {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 14px;
      padding: 14px 16px;
      border-radius: var(--radius-md);
      background: rgba(23, 32, 42, 0.94);
      color: white;
      box-shadow: var(--shadow-md);
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
      box-shadow: none;
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
      grid-template-columns: repeat(2, minmax(0, 1fr));
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
      background: rgba(255, 255, 255, 0.96);
      position: relative;
      overflow: hidden;
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
      transition: border-color 180ms ease, background 180ms ease;
      background: linear-gradient(180deg, rgba(255, 255, 255, 0.86), rgba(252, 247, 239, 0.76));
    }

    .incident-card:hover,
    .incident-card.active {
      border-color: rgba(15, 118, 110, 0.34);
      background: rgba(230, 247, 244, 0.9);
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
      transition: transform 150ms ease, border-color 150ms ease, background 150ms ease, box-shadow 150ms ease;
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
      display: none;
    }

    .skeleton.hero { min-height: 180px; }
    .skeleton.row { min-height: 82px; }
    .skeleton.panel-block { min-height: 210px; }

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

    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after {
        animation: none !important;
        transition: none !important;
        scroll-behavior: auto !important;
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
    }
  </style>
</head>
<body>
  <div class="shell">
    <aside class="panel sidebar" aria-label="Incident list">
      <div class="sidebar-top">
        <div class="eyebrow">Release safety</div>
        <h1>watchdog</h1>
        <p class="sidebar-copy">A Rust incident console that turns deploy regressions into readable evidence. It correlates releases, metric shifts, and new log signatures so developers can understand what changed fast.</p>
      </div>

      <section class="status-banner" aria-label="Monitoring status">
        <div>
          <strong>Monitoring deploy risk</strong>
          <div class="meta" style="color: rgba(255,255,255,0.82);">Live incident view for deployment-linked regressions</div>
        </div>
        <span class="pulse-dot" aria-hidden="true"></span>
      </section>

      <section class="sidebar-stats" aria-label="Dashboard summary">
        <article class="mini-card">
          <div class="label">Incidents</div>
          <strong id="incident-count">0</strong>
        </article>
        <article class="mini-card">
          <div class="label">High Severity</div>
          <strong id="high-count">0</strong>
        </article>
      </section>

      <div id="incident-list" class="incident-list" role="list" aria-label="Incident list"></div>
    </aside>

    <main class="panel detail" id="detail-panel" aria-live="polite"></main>
  </div>

  <script>
    let incidents = [];
    let activeIncidentId = null;

    document.addEventListener('DOMContentLoaded', () => {
      renderEmptyDetail();
      loadIncidents();
    });

    async function loadIncidents() {
      renderIncidentListLoading();
      try {
        const response = await fetch('/api/incidents');
        if (!response.ok) {
          throw new Error(await response.text());
        }
        incidents = await response.json();
        renderSidebarStats();
        renderIncidentList();

        if (incidents.length) {
          const nextId = activeIncidentId && incidents.some((incident) => incident.id === activeIncidentId)
            ? activeIncidentId
            : incidents[0].id;
          await selectIncident(nextId, false);
        } else {
          renderEmptyDetail();
        }
      } catch (error) {
        renderSidebarStats();
        renderIncidentListError(error);
        renderErrorDetail(error);
      }
    }

    function renderSidebarStats() {
      document.getElementById('incident-count').textContent = incidents.length;
      document.getElementById('high-count').textContent = incidents.filter((incident) => incident.severity === 'high').length;
    }

    function renderIncidentListLoading() {
      const container = document.getElementById('incident-list');
      container.innerHTML = Array.from({ length: 3 }).map(() => '<div class="skeleton row" aria-hidden="true"></div>').join('');
    }

    function renderIncidentListError(error) {
      const container = document.getElementById('incident-list');
      container.innerHTML = `<div class="empty">Unable to load incidents. ${escapeHtml(error.message || String(error))}</div>`;
    }

    function renderIncidentList() {
      const container = document.getElementById('incident-list');
      if (!incidents.length) {
        container.innerHTML = '<div class="empty">No incidents captured yet. Start the daemon, trigger a deploy, and this list will populate automatically.</div>';
        return;
      }

      container.innerHTML = incidents.map(renderIncidentCard).join('');
    }

    function renderIncidentCard(incident) {
      const isActive = incident.id === activeIncidentId;
      return `
        <article
          class="incident-card ${isActive ? 'active' : ''}"
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
              ${renderBadge('environment', incident.environment)}
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

    async function selectIncident(id, showLoading = true) {
      activeIncidentId = id;
      renderIncidentList();
      if (showLoading) {
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
        <div class="empty">
          No incidents yet. Run the daemon and trigger a bad deploy simulation to populate the dashboard.
        </div>
      `;
    }

    function renderDetailLoading() {
      document.getElementById('detail-panel').innerHTML = `
        <div class="loading-state" aria-label="Loading incident details">
          <div class="skeleton hero"></div>
          <div class="overview-grid">
            <div class="skeleton row"></div>
            <div class="skeleton row"></div>
            <div class="skeleton row"></div>
            <div class="skeleton row"></div>
          </div>
          <div class="detail-grid">
            <div class="skeleton panel-block"></div>
            <div class="skeleton panel-block"></div>
          </div>
        </div>
      `;
    }

    function renderErrorDetail(error) {
      document.getElementById('detail-panel').innerHTML = `
        <div class="empty">
          Could not load this incident. ${escapeHtml(error.message || String(error))}
        </div>
      `;
    }

    function renderDetail(incident, explanation, loading, explanationError) {
      const panel = document.getElementById('detail-panel');
      const verdict = incident.verdict;
      const comparison = verdict.comparison;
      const signature = verdict.top_error_signature || 'No dominant error signature captured';
      const explanationBody = explanation
        ? escapeHtml(explanation)
        : explanationError
          ? `Explanation failed: ${escapeHtml(explanationError)}`
          : 'No explanation generated yet. Click "Explain Incident" to get an evidence-grounded summary and debugging steps.';

      panel.innerHTML = `
        <section class="hero">
          <div class="hero-copy">
            <div class="badge-row">
              ${renderBadge(incident.severity, incident.severity)}
              ${renderBadge('environment', verdict.environment)}
              ${renderBadge('subtle', `deploy ${escapeHtml(verdict.deploy_id)}`)}
            </div>
            <h2>${escapeHtml(incident.summary)}</h2>
            <p class="subhead">Watchdog flagged this release ${verdict.seconds_after_deploy}s after deploy. The detector saw a statistically meaningful shift in service health and preserved the strongest supporting evidence.</p>
          </div>
          <div class="hero-actions">
            <button class="button button-primary" ${loading ? 'disabled' : ''} onclick="explainIncident('${incident.id}')">${loading ? 'Explaining…' : 'Explain Incident'}</button>
            <button class="button button-secondary" onclick="loadIncidents()">Refresh Incidents</button>
          </div>
        </section>

        <section class="signal-grid" aria-label="Primary signals">
          ${renderSignalCard('Detection Delay', `${verdict.seconds_after_deploy}s`, 'Time between deploy and detected regression', 'warning')}
          ${renderSignalCard('Top Error Signature', signature, verdict.top_error_count ? `Seen ${verdict.top_error_count} times after deploy` : 'No repeated new error count captured', 'danger')}
          ${renderSignalCard('Requests at Detection', `${comparison.request_rate_at_detection.toFixed(1)} req/s`, 'Traffic volume when the verdict was raised', '')}
        </section>

        <section class="overview-grid" aria-label="Incident overview">
          ${renderStatCard('Error Rate Delta', verdict.error_rate_delta.toFixed(3))}
          ${renderStatCard('Latency Delta', `${verdict.latency_delta_ms.toFixed(1)} ms`)}
          ${renderStatCard('Deploy Time', formatDateTime(verdict.deploy_timestamp))}
          ${renderStatCard('Detected At', formatDateTime(verdict.detected_at))}
        </section>

        <section class="detail-grid">
          <div>
            <article class="section-card">
              <div class="section-heading">
                <h3>Before vs After</h3>
                <span class="muted">Baseline against detected state</span>
              </div>
              <div class="compare-grid">
                ${renderCompareCard('Error Rate', comparison.baseline_error_rate.toFixed(3), comparison.detected_error_rate.toFixed(3))}
                ${renderCompareCard('P95 Latency', `${comparison.baseline_latency_ms.toFixed(1)} ms`, `${comparison.detected_latency_ms.toFixed(1)} ms`)}
              </div>
            </article>

            <article class="section-card">
              <div class="section-heading">
                <h3>Incident Timeline</h3>
                <span class="muted">What happened, in order</span>
              </div>
              <div class="timeline">${verdict.timeline.map(renderTimelineItem).join('')}</div>
            </article>
          </div>

          <div>
            <article class="section-card">
              <div class="section-heading">
                <h3>Why Watchdog Flagged It</h3>
                <span class="muted">Detector verdict</span>
              </div>
              <div class="callout">${escapeHtml(incident.alert_text)}</div>
            </article>

            <article class="section-card">
              <div class="section-heading">
                <h3>Dominant Error Signature</h3>
                <span class="muted">Most repeated new error</span>
              </div>
              <div class="signature-chip">
                <strong>${escapeHtml(signature)}</strong>
                ${verdict.top_error_count ? `<span>Seen ${verdict.top_error_count} times after deploy</span>` : '<span>No repeated count available</span>'}
              </div>
            </article>

            <article class="section-card">
              <div class="section-heading">
                <h3>AI Explanation</h3>
                <span class="muted">Evidence-grounded summary</span>
              </div>
              <pre>${explanationBody}</pre>
            </article>
          </div>
        </section>
      `;
    }

    async function explainIncident(id) {
      let incident;
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
      }
    }

    function renderBadge(kind, label) {
      return `<span class="badge ${kind}">${escapeHtml(label)}</span>`;
    }

    function renderStatCard(label, value) {
      return `
        <article class="stat-card">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
        </article>
      `;
    }

    function renderSignalCard(label, value, detail, tone) {
      return `
        <article class="signal-card ${tone}">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
          <p class="meta" style="margin-top: 8px;">${escapeHtml(detail)}</p>
        </article>
      `;
    }

    function renderCompareCard(title, baseline, detected) {
      return `
        <div class="compare-card">
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
        <article class="timeline-item">
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
