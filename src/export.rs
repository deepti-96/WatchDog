use crate::model::Incident;

pub fn render_markdown(incident: &Incident) -> String {
    let verdict = &incident.verdict;
    let top_error = verdict
        .top_error_signature
        .as_deref()
        .unwrap_or("No dominant error signature captured");
    let cached_note = incident
        .cached_explanation_updated_at
        .map(|ts| format!("Cached at {}", ts))
        .unwrap_or_else(|| "No cached AI explanation".to_string());

    let timeline = verdict
        .timeline
        .iter()
        .map(|event| format!("- **{}** ({}) - {}", event.label, event.timestamp, event.detail))
        .collect::<Vec<_>>()
        .join("\n");

    let explanation = incident
        .cached_explanation
        .as_deref()
        .unwrap_or("No AI explanation has been generated for this incident yet.");

    let notes = if incident.notes.trim().is_empty() {
        "No investigation notes recorded yet.".to_string()
    } else {
        incident.notes.clone()
    };

    format!(
        "# Watchdog Incident Report\n\n## Summary\n- Incident ID: `{}`\n- Deploy: `{}`\n- Environment: `{}`\n- Severity: `{}`\n- Status: `{}`\n- Created At: `{}`\n- Detected At: `{}`\n- Seconds After Deploy: `{}`\n\n## Regression Signals\n- Error Rate Delta: `{:.3}`\n- Latency Delta: `{:.1} ms`\n- Requests at Detection: `{:.1} req/s`\n- Dominant Error Signature: `{}`\n- Dominant Error Count: `{}`\n\n## Detector Verdict\n{}\n\n## Timeline\n{}\n\n## Investigation Notes\n{}\n\n## AI Explanation\n_{}_\n\n{}\n",
        incident.id,
        verdict.deploy_id,
        verdict.environment,
        incident.severity,
        incident.status,
        incident.created_at,
        verdict.detected_at,
        verdict.seconds_after_deploy,
        verdict.error_rate_delta,
        verdict.latency_delta_ms,
        verdict.comparison.request_rate_at_detection,
        top_error,
        verdict.top_error_count,
        incident.alert_text,
        timeline,
        notes,
        cached_note,
        explanation
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Incident, IncidentMetricComparison, IncidentTimelineEvent, RegressionVerdict};
    use chrono::Utc;

    #[test]
    fn markdown_export_contains_core_incident_fields() {
        let incident = Incident {
            id: "abc-123".to_string(),
            created_at: Utc::now(),
            severity: "high".to_string(),
            summary: "demo regression".to_string(),
            verdict: RegressionVerdict {
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
            cached_explanation: Some("Likely DB pool exhaustion".to_string()),
            cached_explanation_updated_at: Some(Utc::now()),
            status: "open".to_string(),
            notes: "Check DB pool metrics".to_string(),
        };

        let markdown = render_markdown(&incident);
        assert!(markdown.contains("Watchdog Incident Report"));
        assert!(markdown.contains("v1.2.3"));
        assert!(markdown.contains("api: database timeout"));
        assert!(markdown.contains("Likely DB pool exhaustion"));
        assert!(markdown.contains("Check DB pool metrics"));
        assert!(markdown.contains("Status: `open`"));
    }
}
