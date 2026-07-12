# Production Readiness Notes

WatchDog is a deploy-regression detection daemon prototype. The Rust engine, alert rendering, log signature extraction, dashboard API, storage adapters, and tests are implemented in this repository. The default ingestion path uses local JSONL files so the full workflow can be reviewed and tested without external infrastructure.

## What is production-shaped today

- Deploy correlation is explicit: a deploy event snapshots the current baseline and opens a bounded monitoring window.
- Detection is deterministic: CUSUM tracks sustained error-rate and latency shifts above the pre-deploy baseline.
- Incidents are evidence-backed: each incident stores metric deltas, timing, dominant log signature, status, notes, and exportable summaries.
- Storage is replaceable: JSON files are useful for local demos, SQLite works for local durable runs, and Supabase/PostgREST supports hosted demos.
- The dashboard exposes a health endpoint and incident APIs, which makes the service easy to smoke test in CI/CD or a hosted environment.

## Current limitations

- Metrics and deploy events are ingested from JSONL in the MVP. A production deployment should replace this with Prometheus, OpenTelemetry, deploy webhooks, or CI/CD platform events.
- The baseline is a rolling mean over recent samples. It does not yet model seasonality, traffic class, regional differences, or day-over-day patterns.
- Only one active deploy is tracked at a time. High-frequency deploy environments would need a queue of active deploy windows or tighter release metadata.
- Thresholds are static unless configured. Production use should tune thresholds per service and measure precision/recall from historical incidents.
- The daemon records and explains evidence, but it deliberately does not perform automatic rollback. Rollback should stay behind a human approval gate.

## How I would evolve it for a real production environment

1. Add a metrics adapter trait with Prometheus and OpenTelemetry implementations.
2. Accept deploy events from GitHub Actions, Kubernetes rollout annotations, or an internal deploy service.
3. Store historical outcomes so teams can tune thresholds and track false positives.
4. Support multiple concurrent deploy windows keyed by service, environment, and deploy id.
5. Emit structured logs and service metrics for WatchDog itself, including detection latency, incidents opened, webhook failures, and storage errors.
6. Add runbook links, service ownership, and rollback checklist metadata to the incident model.
