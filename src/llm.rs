use crate::model::{Incident, IncidentMetricComparison, IncidentTimelineEvent};
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
}
