const TABLE = 'incidents';

function requireEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required`);
  }
  return value;
}

function supabaseBaseUrl() {
  return requireEnv('SUPABASE_URL').replace(/\/$/, '');
}

function supabaseHeaders(extra = {}) {
  const key = requireEnv('SUPABASE_SERVICE_ROLE_KEY');
  return {
    apikey: key,
    Authorization: `Bearer ${key}`,
    'Content-Type': 'application/json',
    ...extra,
  };
}

async function supabaseFetch(path, options = {}) {
  const response = await fetch(`${supabaseBaseUrl()}/rest/v1/${path}`, {
    ...options,
    headers: supabaseHeaders(options.headers || {}),
  });
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`Supabase ${response.status}: ${body}`);
  }
  return response;
}

function listItem(incident) {
  return {
    id: incident.id,
    created_at: incident.created_at,
    severity: incident.severity,
    summary: incident.summary,
    deploy_id: incident.verdict.deploy_id,
    environment: incident.verdict.environment,
    has_cached_explanation: Boolean(incident.cached_explanation),
    has_agent_report: Boolean(incident.agent_report),
    status: incident.status || 'open',
    has_notes: Boolean((incident.notes || '').trim()),
  };
}

async function listIncidents() {
  const response = await supabaseFetch(`${TABLE}?select=incident_json&order=created_at.desc`);
  const rows = await response.json();
  return rows.map((row) => row.incident_json);
}

async function readIncident(id) {
  const response = await supabaseFetch(`${TABLE}?select=incident_json&id=eq.${encodeURIComponent(id)}&limit=1`);
  const rows = await response.json();
  return rows[0]?.incident_json || null;
}

async function writeIncident(incident) {
  const row = {
    id: incident.id,
    created_at: incident.created_at,
    severity: incident.severity,
    status: incident.status || 'open',
    deploy_id: incident.verdict.deploy_id,
    environment: incident.verdict.environment,
    summary: incident.summary,
    incident_json: incident,
    updated_at: new Date().toISOString(),
  };
  await supabaseFetch(`${TABLE}?on_conflict=id`, {
    method: 'POST',
    headers: { Prefer: 'resolution=merge-duplicates,return=minimal' },
    body: JSON.stringify(row),
  });
  return incident;
}

function normalizeSignature(service, message) {
  return `${service}: ${message.toLowerCase()}`
    .replace(/[a-f0-9]{8,}/g, '<id>')
    .replace(/\b\d+\b/g, '<num>');
}

function createScenarioIncident(scenario = 'checkout-timeout') {
  const now = Date.now();
  const patch = 20 + (Math.floor(now / 1000) % 70);
  const deployId = `v3.2.${patch}`;
  const previousDeployId = `v3.2.${patch - 1}`;
  const environment = 'production';
  const deployAt = new Date(now + 31_000);
  const detectedAt = new Date(now + 35_000);
  const isPayments = scenario === 'payments-latency';
  const service = isPayments ? 'payments-api' : 'checkout-api';
  const rawMessage = isPayments
    ? 'Payment provider timeout while authorizing card 4242 request 8f91ab22 after release v3.2'
    : 'Database timeout while loading checkout session user 123 request 8f91ab22 after release v3.2';
  const signature = normalizeSignature(service, rawMessage);
  const baselineErrorRate = 0.012;
  const detectedErrorRate = isPayments ? 0.051 : 0.128;
  const baselineLatencyMs = 117.7;
  const detectedLatencyMs = isPayments ? 393.0 : 293.0;
  const reason = isPayments ? 'latency shifted above baseline' : 'error rate and latency shifted above baseline';
  const id = `${Math.floor(detectedAt.getTime() / 1000)}-${deployId.replace(/[^a-zA-Z0-9]+/g, '-').toLowerCase()}`;
  const verdict = {
    deploy_id: deployId,
    environment,
    deploy_timestamp: deployAt.toISOString(),
    detected_at: detectedAt.toISOString(),
    seconds_after_deploy: 4,
    error_rate_delta: detectedErrorRate - baselineErrorRate,
    latency_delta_ms: detectedLatencyMs - baselineLatencyMs,
    reason,
    top_error_signature: signature,
    top_error_count: 1,
    top_error_is_new: true,
    comparison: {
      baseline_error_rate: baselineErrorRate,
      detected_error_rate: detectedErrorRate,
      baseline_latency_ms: baselineLatencyMs,
      detected_latency_ms: detectedLatencyMs,
      request_rate_at_detection: 405.0,
    },
    timeline: [
      { label: 'Previous release stable', timestamp: new Date(now - 10 * 60_000).toISOString(), detail: `${previousDeployId} held baseline at ${(baselineErrorRate * 100).toFixed(1)}% errors and ${baselineLatencyMs.toFixed(1)}ms p95` },
      { label: 'Production deploy started', timestamp: deployAt.toISOString(), detail: `${deployId} promoted to ${environment} for ${service}` },
      { label: 'First dominant error', timestamp: detectedAt.toISOString(), detail: signature },
      { label: 'Regression detected', timestamp: detectedAt.toISOString(), detail: `${deployId} crossed the release guardrail: ${reason}` },
    ],
  };
  return {
    id,
    created_at: new Date().toISOString(),
    severity: 'high',
    summary: `${deployId} regressed ${service} in production after ${previousDeployId} baseline`,
    verdict,
    alert_text: `watchdog detected a production deployment regression: ${deployId} replaced stable ${previousDeployId} and triggered ${reason} 4s later. error rate moved from ${(baselineErrorRate * 100).toFixed(1)}% to ${(detectedErrorRate * 100).toFixed(1)}%; p95 latency moved from ${baselineLatencyMs.toFixed(1)}ms to ${detectedLatencyMs.toFixed(1)}ms. Dominant new error after deploy: '${signature}' seen 1 times.`,
    cached_explanation: null,
    cached_explanation_updated_at: null,
    status: 'open',
    notes: '',
  };
}

function explainIncident(incident) {
  const verdict = incident.verdict;
  const comparison = verdict.comparison;
  const errorMultiplier = comparison.baseline_error_rate > 0
    ? comparison.detected_error_rate / comparison.baseline_error_rate
    : 0;
  const latencyMultiplier = comparison.baseline_latency_ms > 0
    ? comparison.detected_latency_ms / comparison.baseline_latency_ms
    : 0;
  return `## Likely Issue
Deploy \`${verdict.deploy_id}\` likely introduced a backend regression in \`${verdict.environment}\`. WatchDog detected \`${verdict.reason}\` ${verdict.seconds_after_deploy}s after the release.

## Why
- Error rate moved from ${comparison.baseline_error_rate.toFixed(3)} to ${comparison.detected_error_rate.toFixed(3)} (${errorMultiplier.toFixed(1)}x baseline).
- P95 latency moved from ${comparison.baseline_latency_ms.toFixed(1)}ms to ${comparison.detected_latency_ms.toFixed(1)}ms (${latencyMultiplier.toFixed(1)}x baseline).
- A new post-deploy log signature appeared: \`${verdict.top_error_signature}\`.

## Next Steps
- Check the deploy diff for database, timeout, connection pool, or API handler changes.
- Inspect traces/logs around the first post-deploy error timestamp.
- Roll back or gate the release if customer-facing impact is still rising.

## Confidence
High based on deploy timing, metric deltas, and log evidence.`;
}

function buildAgentReport(incident) {
  const verdict = incident.verdict;
  const comparison = verdict.comparison;
  const errorMultiplier = comparison.baseline_error_rate > 0
    ? comparison.detected_error_rate / comparison.baseline_error_rate
    : 0;
  const latencyMultiplier = comparison.baseline_latency_ms > 0
    ? comparison.detected_latency_ms / comparison.baseline_latency_ms
    : 0;
  const confidence = verdict.top_error_is_new && (errorMultiplier >= 4 || latencyMultiplier >= 2)
    ? 'high'
    : 'medium';
  const shouldRollback = verdict.error_rate_delta >= 0.08 || verdict.latency_delta_ms >= 200;
  const action = shouldRollback
    ? 'Gate or roll back the release while the owning service checks the deploy diff.'
    : 'Keep the release under elevated watch and inspect traces for the dominant signature.';

  return {
    generated_at: new Date().toISOString(),
    audit_status: 'stored in Supabase incident_json',
    confidence,
    hypothesis: `${verdict.deploy_id} likely regressed ${verdict.environment} because post-deploy health diverged from the previous stable baseline within ${verdict.seconds_after_deploy}s.`,
    recommended_action: action,
    evidence_used: [
      `deploy: ${verdict.deploy_id}`,
      `environment: ${verdict.environment}`,
      `baseline error rate: ${comparison.baseline_error_rate.toFixed(3)}`,
      `detected error rate: ${comparison.detected_error_rate.toFixed(3)}`,
      `baseline p95 latency: ${comparison.baseline_latency_ms.toFixed(1)}ms`,
      `detected p95 latency: ${comparison.detected_latency_ms.toFixed(1)}ms`,
      `top error signature: ${verdict.top_error_signature || 'none captured'}`,
    ],
    next_checks: [
      'Compare the deploy diff against the service owning the dominant signature.',
      'Inspect traces and logs from the first post-deploy error timestamp.',
      'Confirm whether rollback returns error rate and latency to baseline.',
    ],
    limitations: [
      'This agent only uses the stored incident evidence shown in this dashboard.',
      'It does not inspect source code, distributed traces, customer tickets, or cloud provider status.',
      'The demo release inputs are generated, while persistence, status, notes, explanations, and agent reports are real Supabase-backed records.',
    ],
  };
}

function autonomouslyTriageIncident(incident) {
  const triaged = {
    ...incident,
    cached_explanation: incident.cached_explanation || explainIncident(incident),
    cached_explanation_updated_at: incident.cached_explanation_updated_at || new Date().toISOString(),
  };
  triaged.agent_report = buildAgentReport(triaged);
  triaged.agent_report_updated_at = new Date().toISOString();
  triaged.autonomous_run = {
    mode: 'deploy-webhook',
    completed_at: new Date().toISOString(),
    actions: [
      'accepted production deploy event',
      'compared post-deploy health against previous stable baseline',
      'opened incident after guardrail breach',
      'generated evidence explanation',
      'generated triage recommendation',
      'persisted audit trail to Supabase',
    ],
    guardrails: [
      'does not auto-rollback production',
      'does not claim evidence outside the stored incident',
      'keeps Supabase service role server-side',
    ],
  };
  return triaged;
}

function sendJson(res, status, body) {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json');
  res.end(JSON.stringify(body));
}

function sendError(res, error) {
  sendJson(res, 500, { error: error.message || String(error) });
}

module.exports = {
  autonomouslyTriageIncident,
  buildAgentReport,
  createScenarioIncident,
  explainIncident,
  listIncidents,
  listItem,
  readIncident,
  sendError,
  sendJson,
  writeIncident,
};
