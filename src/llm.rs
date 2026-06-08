use crate::model::Incident;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

#[derive(Debug, Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: String,
    system: &'a str,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
}

pub async fn explain_incident(incident: &Incident) -> Result<String> {
    let mode = env::var("WATCHDOG_EXPLAINER").unwrap_or_else(|_| "auto".to_string());
    if mode.eq_ignore_ascii_case("local") {
        return Ok(explain_incident_locally(incident));
    }

    match explain_incident_with_ollama(incident).await {
        Ok(explanation) => Ok(explanation),
        Err(error) if mode.eq_ignore_ascii_case("auto") => Ok(format!(
            "{}\n\n_Note: local lightweight explanation used because Ollama was unavailable: {}_",
            explain_incident_locally(incident),
            error
        )),
        Err(error) => Err(error),
    }
}

async fn explain_incident_with_ollama(incident: &Incident) -> Result<String> {
    let base_url = env::var("WATCHDOG_OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434/api".to_string());
    let model = env::var("WATCHDOG_OLLAMA_MODEL").unwrap_or_else(|_| "gemma3".to_string());

    let request = OllamaRequest {
        model: &model,
        prompt: build_incident_prompt(incident),
        system: "You are Watchdog, an incident explanation assistant for deployment regressions. Use only the provided evidence. Do not invent facts. Explain the likely issue, why the evidence points there, and the first debugging steps. If evidence is limited, say so explicitly. Format the response in short Markdown with these headings: Likely Issue, Why, Next Steps, Confidence.",
        stream: false,
    };

    let endpoint = format!("{}/generate", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(&endpoint)
        .json(&request)
        .send()
        .await
        .with_context(|| format!("failed to reach Ollama at {}. Start Ollama first or set WATCHDOG_OLLAMA_BASE_URL", endpoint))?
        .error_for_status()?
        .json::<OllamaResponse>()
        .await?;

    let text = response.response.trim().to_string();
    if text.is_empty() {
        Err(anyhow!("Ollama returned an empty explanation"))
    } else {
        Ok(text)
    }
}

fn explain_incident_locally(incident: &Incident) -> String {
    let verdict = &incident.verdict;
    let comparison = &verdict.comparison;
    let error_multiplier = if comparison.baseline_error_rate > 0.0 {
        comparison.detected_error_rate / comparison.baseline_error_rate
    } else {
        0.0
    };
    let latency_multiplier = if comparison.baseline_latency_ms > 0.0 {
        comparison.detected_latency_ms / comparison.baseline_latency_ms
    } else {
        0.0
    };

    let error_signal = match &verdict.top_error_signature {
        Some(signature) if verdict.top_error_is_new => format!(
            "A new post-deploy log signature appeared: `{signature}`. It was seen {} time(s), which makes it a strong first debugging target.",
            verdict.top_error_count
        ),
        Some(signature) => format!(
            "The dominant post-deploy log signature was `{signature}`, seen {} time(s). It existed before, so compare its post-deploy frequency against the baseline.",
            verdict.top_error_count
        ),
        None => "No dominant log signature was captured, so the strongest evidence is the metric shift.".to_string(),
    };

    let confidence = if verdict.top_error_signature.is_some()
        && verdict.error_rate_delta > 0.05
        && verdict.latency_delta_ms > 100.0
    {
        "High"
    } else if verdict.error_rate_delta > 0.03 || verdict.latency_delta_ms > 80.0 {
        "Medium"
    } else {
        "Low"
    };

    format!(
        r#"## Likely Issue
Deploy `{}` likely introduced a backend regression in `{}`. WatchDog detected `{}` {}s after the release.

## Why
- Error rate moved from {:.3} to {:.3} ({:.1}x baseline).
- P95 latency moved from {:.1}ms to {:.1}ms ({:.1}x baseline).
- {}

## Next Steps
- Check the deploy diff for database, timeout, connection pool, or API handler changes.
- Inspect traces/logs around the first post-deploy error timestamp.
- Roll back or gate the release if customer-facing impact is still rising.
- After mitigation, rerun WatchDog against the same deploy window to confirm error rate and latency return to baseline.

## Confidence
{} based on deploy timing, metric deltas, and available log evidence."#,
        verdict.deploy_id,
        verdict.environment,
        verdict.reason,
        verdict.seconds_after_deploy,
        comparison.baseline_error_rate,
        comparison.detected_error_rate,
        error_multiplier,
        comparison.baseline_latency_ms,
        comparison.detected_latency_ms,
        latency_multiplier,
        error_signal,
        confidence
    )
}

fn build_incident_prompt(incident: &Incident) -> String {
    json!({
        "incident_id": incident.id,
        "severity": incident.severity,
        "summary": incident.summary,
        "alert_text": incident.alert_text,
        "deploy_id": incident.verdict.deploy_id,
        "environment": incident.verdict.environment,
        "deploy_timestamp": incident.verdict.deploy_timestamp,
        "detected_at": incident.verdict.detected_at,
        "seconds_after_deploy": incident.verdict.seconds_after_deploy,
        "error_rate_delta": incident.verdict.error_rate_delta,
        "latency_delta_ms": incident.verdict.latency_delta_ms,
        "reason": incident.verdict.reason,
        "top_error_signature": incident.verdict.top_error_signature,
        "top_error_count": incident.verdict.top_error_count,
        "top_error_is_new": incident.verdict.top_error_is_new,
        "comparison": incident.verdict.comparison,
        "timeline": incident.verdict.timeline
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IncidentMetricComparison, IncidentTimelineEvent};
    use chrono::Utc;

    #[test]
    fn incident_prompt_contains_core_fields() {
        let incident = Incident {
            id: "123-demo".to_string(),
            created_at: Utc::now(),
            severity: "high".to_string(),
            summary: "demo regression".to_string(),
            verdict: crate::model::RegressionVerdict {
                deploy_id: "v1.2.3".to_string(),
                environment: "demo".to_string(),
                deploy_timestamp: Utc::now(),
                detected_at: Utc::now(),
                seconds_after_deploy: 4,
                error_rate_delta: 0.1,
                latency_delta_ms: 150.0,
                reason: "error rate and latency shifted above baseline".to_string(),
                top_error_signature: Some("api: database timeout".to_string()),
                top_error_count: 3,
                top_error_is_new: true,
                comparison: IncidentMetricComparison {
                    baseline_error_rate: 0.01,
                    detected_error_rate: 0.11,
                    baseline_latency_ms: 110.0,
                    detected_latency_ms: 260.0,
                    request_rate_at_detection: 405.0,
                },
                timeline: vec![IncidentTimelineEvent {
                    label: "Regression detected".to_string(),
                    timestamp: Utc::now(),
                    detail: "error rate and latency shifted above baseline".to_string(),
                }],
            },
            alert_text: "watchdog detected a deployment regression".to_string(),
            cached_explanation: None,
            cached_explanation_updated_at: None,
            status: "open".to_string(),
            notes: String::new(),
        };

        let prompt = build_incident_prompt(&incident);
        assert!(prompt.contains("v1.2.3"));
        assert!(prompt.contains("database timeout"));
        assert!(prompt.contains("seconds_after_deploy"));
        assert!(prompt.contains("comparison"));
        assert!(prompt.contains("timeline"));
    }

    #[test]
    fn local_explainer_generates_markdown_from_incident_evidence() {
        let incident = Incident {
            id: "123-demo".to_string(),
            created_at: Utc::now(),
            severity: "high".to_string(),
            summary: "demo regression".to_string(),
            verdict: crate::model::RegressionVerdict {
                deploy_id: "v1.2.3".to_string(),
                environment: "production".to_string(),
                deploy_timestamp: Utc::now(),
                detected_at: Utc::now(),
                seconds_after_deploy: 5,
                error_rate_delta: 0.098,
                latency_delta_ms: 142.8,
                reason: "error rate and latency shifted above baseline".to_string(),
                top_error_signature: Some("api: database timeout".to_string()),
                top_error_count: 3,
                top_error_is_new: true,
                comparison: IncidentMetricComparison {
                    baseline_error_rate: 0.012,
                    detected_error_rate: 0.11,
                    baseline_latency_ms: 117.2,
                    detected_latency_ms: 260.0,
                    request_rate_at_detection: 405.0,
                },
                timeline: vec![IncidentTimelineEvent {
                    label: "Regression detected".to_string(),
                    timestamp: Utc::now(),
                    detail: "error rate and latency shifted above baseline".to_string(),
                }],
            },
            alert_text: "watchdog detected a deployment regression".to_string(),
            cached_explanation: None,
            cached_explanation_updated_at: None,
            status: "open".to_string(),
            notes: String::new(),
        };

        let explanation = explain_incident_locally(&incident);
        assert!(explanation.contains("## Likely Issue"));
        assert!(explanation.contains("v1.2.3"));
        assert!(explanation.contains("api: database timeout"));
        assert!(explanation.contains("## Next Steps"));
    }
}
