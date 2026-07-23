const TABLE = 'incidents';
const DEMO_ENVIRONMENT = 'production';
const DETECTION_DELAY_SECONDS = 4;
const PREVIOUS_RELEASE_STABLE_MINUTES = 10;
const REQUEST_RATE_AT_DETECTION = 405.0;
const ROLLBACK_ERROR_DELTA_THRESHOLD = 0.08;
const ROLLBACK_LATENCY_DELTA_MS_THRESHOLD = 200;
const ROLLBACK_ERROR_MULTIPLIER_THRESHOLD = 5;
const ROLLBACK_LATENCY_MULTIPLIER_THRESHOLD = 2;
const SCENARIOS = {
  'checkout-timeout': {
    service: 'checkout-api',
    raw_message: 'Database timeout while loading checkout session user 123 request 8f91ab22 after release v3.2',
    detected_error_rate: 0.128,
    detected_latency_ms: 293.0,
    reason: 'error rate and latency shifted above baseline',
  },
  'payments-latency': {
    service: 'payments-api',
    raw_message: 'Payment provider timeout while authorizing card 4242 request 8f91ab22 after release v3.2',
    detected_error_rate: 0.051,
    detected_latency_ms: 393.0,
    reason: 'latency shifted above baseline',
  },
};

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

function ratioOrZero(numerator, denominator) {
  return denominator > 0 ? numerator / denominator : 0;
}

function createScenarioIncident(scenario = 'checkout-timeout') {
  const fixture = SCENARIOS[scenario] || SCENARIOS['checkout-timeout'];
  const now = Date.now();
  const patch = 20 + (Math.floor(now / 1000) % 70);
  const deployId = `v3.2.${patch}`;
  const previousDeployId = `v3.2.${patch - 1}`;
  const environment = DEMO_ENVIRONMENT;
  const deployAt = new Date(now + 31_000);
  const detectedAt = new Date(now + 35_000);
  const service = fixture.service;
  const signature = normalizeSignature(service, fixture.raw_message);
  const baselineErrorRate = 0.012;
  const detectedErrorRate = fixture.detected_error_rate;
  const baselineLatencyMs = 117.7;
  const detectedLatencyMs = fixture.detected_latency_ms;
  const reason = fixture.reason;
  const id = `${Math.floor(detectedAt.getTime() / 1000)}-${deployId.replace(/[^a-zA-Z0-9]+/g, '-').toLowerCase()}`;
  const verdict = {
    deploy_id: deployId,
    environment,
    deploy_timestamp: deployAt.toISOString(),
    detected_at: detectedAt.toISOString(),
    seconds_after_deploy: DETECTION_DELAY_SECONDS,
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
      request_rate_at_detection: REQUEST_RATE_AT_DETECTION,
    },
    timeline: [
      { label: 'Previous release stable', timestamp: new Date(now - PREVIOUS_RELEASE_STABLE_MINUTES * 60_000).toISOString(), detail: `${previousDeployId} held baseline at ${(baselineErrorRate * 100).toFixed(1)}% errors and ${baselineLatencyMs.toFixed(1)}ms p95` },
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
    alert_text: `watchdog detected a production deployment regression: ${deployId} replaced stable ${previousDeployId} and triggered ${reason} ${DETECTION_DELAY_SECONDS}s later. error rate moved from ${(baselineErrorRate * 100).toFixed(1)}% to ${(detectedErrorRate * 100).toFixed(1)}%; p95 latency moved from ${baselineLatencyMs.toFixed(1)}ms to ${detectedLatencyMs.toFixed(1)}ms. Dominant new error after deploy: '${signature}' seen 1 times.`,
    cached_explanation: null,
    cached_explanation_updated_at: null,
    status: 'open',
    notes: '',
  };
}

function explainIncident(incident) {
  const verdict = incident.verdict;
  const comparison = verdict.comparison;
  const errorMultiplier = ratioOrZero(comparison.detected_error_rate, comparison.baseline_error_rate);
  const latencyMultiplier = ratioOrZero(comparison.detected_latency_ms, comparison.baseline_latency_ms);
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
  const errorMultiplier = ratioOrZero(comparison.detected_error_rate, comparison.baseline_error_rate);
  const latencyMultiplier = ratioOrZero(comparison.detected_latency_ms, comparison.baseline_latency_ms);
  const confidence = verdict.top_error_is_new && (errorMultiplier >= 4 || latencyMultiplier >= 2)
    ? 'high'
    : 'medium';
  const shouldRollback = verdict.error_rate_delta >= ROLLBACK_ERROR_DELTA_THRESHOLD
    || verdict.latency_delta_ms >= ROLLBACK_LATENCY_DELTA_MS_THRESHOLD;
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

function buildRollbackBrief(incident) {
  const verdict = incident.verdict;
  const comparison = verdict.comparison;
  const errorMultiplier = ratioOrZero(comparison.detected_error_rate, comparison.baseline_error_rate);
  const latencyMultiplier = ratioOrZero(comparison.detected_latency_ms, comparison.baseline_latency_ms);
  const rollbackRecommended = verdict.error_rate_delta >= ROLLBACK_ERROR_DELTA_THRESHOLD
    || verdict.latency_delta_ms >= ROLLBACK_LATENCY_DELTA_MS_THRESHOLD
    || errorMultiplier >= ROLLBACK_ERROR_MULTIPLIER_THRESHOLD
    || latencyMultiplier >= ROLLBACK_LATENCY_MULTIPLIER_THRESHOLD;

  return {
    generated_at: new Date().toISOString(),
    decision: rollbackRecommended ? 'rollback recommended' : 'hold and monitor',
    owner: verdict.top_error_signature?.split(':')[0] || 'service owner',
    blast_radius: `${verdict.environment} traffic at ${comparison.request_rate_at_detection.toFixed(1)} req/s`,
    customer_risk: rollbackRecommended
      ? 'Customer-facing risk is likely because the new release crossed latency or error-rate guardrails shortly after deploy.'
      : 'Customer-facing risk is possible but below the automatic rollback recommendation threshold.',
    rollback_trigger: [
      `error rate ${comparison.detected_error_rate.toFixed(3)} vs baseline ${comparison.baseline_error_rate.toFixed(3)} (${errorMultiplier.toFixed(1)}x)`,
      `p95 latency ${comparison.detected_latency_ms.toFixed(1)}ms vs baseline ${comparison.baseline_latency_ms.toFixed(1)}ms (${latencyMultiplier.toFixed(1)}x)`,
      `dominant new signature: ${verdict.top_error_signature || 'none captured'}`,
    ],
    operator_steps: rollbackRecommended
      ? [
        `Pause rollout for ${verdict.deploy_id}.`,
        'Notify the owning service channel with the incident link.',
        'Roll back to the previous stable release if the signal is still elevated.',
        'Keep WatchDog open until metrics return to baseline.',
      ]
      : [
        'Keep the release under elevated watch.',
        'Ask the owning service to inspect traces around the first post-deploy error.',
        'Escalate to rollback if the next monitoring window worsens.',
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
  triaged.rollback_brief = buildRollbackBrief(triaged);
  triaged.rollback_brief_updated_at = new Date().toISOString();
  triaged.autonomous_run = {
    mode: 'deploy-webhook',
    completed_at: new Date().toISOString(),
    actions: [
      'accepted production deploy event',
      'compared post-deploy health against previous stable baseline',
      'opened incident after guardrail breach',
      'generated evidence explanation',
      'generated triage recommendation',
      'prepared rollback decision brief',
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
  buildRollbackBrief,
  createScenarioIncident,
  explainIncident,
  listIncidents,
  listItem,
  readIncident,
  sendError,
  sendJson,
  writeIncident,
};
