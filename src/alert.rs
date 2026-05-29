use crate::model::RegressionVerdict;
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct WebhookPayload<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<Vec<Value>>,
}

pub fn render(verdict: &RegressionVerdict) -> String {
    let error_context = match &verdict.top_error_signature {
        Some(signature) if verdict.top_error_is_new => format!(
            " Dominant new error after deploy: '{}' seen {} times.",
            signature, verdict.top_error_count
        ),
        Some(signature) => format!(
            " Dominant post-deploy error signature: '{}' seen {} times.",
            signature, verdict.top_error_count
        ),
        None => String::new(),
    };

    format!(
        "watchdog detected a deployment regression: deploy {} triggered {} {}s later. error rate delta: {:.3}, latency delta: {:.1}ms, detected at {}.{}",
        verdict.deploy_id,
        verdict.reason,
        verdict.seconds_after_deploy,
        verdict.error_rate_delta,
        verdict.latency_delta_ms,
        verdict.detected_at,
        error_context,
    )
}

pub async fn send_webhook(webhook_url: &str, body: &str, verdict: &RegressionVerdict) -> Result<()> {
    let client = reqwest::Client::new();
    client
        .post(webhook_url)
        .json(&build_webhook_payload(webhook_url, body, verdict))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn build_webhook_payload<'a>(
    webhook_url: &str,
    body: &'a str,
    verdict: &RegressionVerdict,
) -> WebhookPayload<'a> {
    WebhookPayload {
        text: body,
        blocks: is_slack_webhook(webhook_url).then(|| render_slack_blocks(verdict)),
    }
}

fn is_slack_webhook(webhook_url: &str) -> bool {
    webhook_url.contains("hooks.slack.com/")
}

fn render_slack_blocks(verdict: &RegressionVerdict) -> Vec<Value> {
    let top_error = match &verdict.top_error_signature {
        Some(signature) if verdict.top_error_is_new => {
            format!("New error: `{}` seen {} times", signature, verdict.top_error_count)
        }
        Some(signature) => {
            format!("Post-deploy error: `{}` seen {} times", signature, verdict.top_error_count)
        }
        None => "No dominant error signature captured".to_string(),
    };

    let timeline = verdict
        .timeline
        .iter()
        .map(|event| {
            format!(
                "* {} - {}: {}",
                event.timestamp.format("%H:%M:%S UTC"),
                event.label,
                event.detail
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    vec![
        json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": "WatchDog regression detected",
                "emoji": true
            }
        }),
        json!({
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": format!("*{}* in `{}` triggered *{}*.", verdict.deploy_id, verdict.environment, verdict.reason)
            }
        }),
        json!({
            "type": "section",
            "fields": [
                { "type": "mrkdwn", "text": format!("*Detected after*\n{}s", verdict.seconds_after_deploy) },
                { "type": "mrkdwn", "text": format!("*Detected at*\n{}", verdict.detected_at.format("%Y-%m-%d %H:%M:%S UTC")) },
                { "type": "mrkdwn", "text": format!("*Error delta*\n{:.3}", verdict.error_rate_delta) },
                { "type": "mrkdwn", "text": format!("*Latency delta*\n{:.1}ms", verdict.latency_delta_ms) },
                { "type": "mrkdwn", "text": format!("*Requests*\n{:.1} req/s", verdict.comparison.request_rate_at_detection) },
                { "type": "mrkdwn", "text": format!("*Error signal*\n{}", top_error) }
            ]
        }),
        json!({ "type": "divider" }),
        json!({
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": format!("*Timeline*\n{}", if timeline.is_empty() { "No timeline events captured.".to_string() } else { timeline })
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IncidentMetricComparison, IncidentTimelineEvent};
    use chrono::{Duration, Utc};

    fn sample_verdict() -> RegressionVerdict {
        let start = Utc::now();
        RegressionVerdict {
            deploy_id: "v2.1.0".to_string(),
            environment: "production".to_string(),
            deploy_timestamp: start,
            detected_at: start + Duration::seconds(4),
            seconds_after_deploy: 4,
            error_rate_delta: 0.08,
            latency_delta_ms: 144.0,
            reason: "error rate and latency shifted above baseline".to_string(),
            top_error_signature: Some("api: database timeout".to_string()),
            top_error_count: 3,
            top_error_is_new: true,
            comparison: IncidentMetricComparison {
                baseline_error_rate: 0.01,
                detected_error_rate: 0.09,
                baseline_latency_ms: 110.0,
                detected_latency_ms: 254.0,
                request_rate_at_detection: 405.0,
            },
            timeline: vec![
                IncidentTimelineEvent {
                    label: "Deploy started".to_string(),
                    timestamp: start,
                    detail: "v2.1.0 deployed to production".to_string(),
                },
                IncidentTimelineEvent {
                    label: "Regression detected".to_string(),
                    timestamp: start + Duration::seconds(4),
                    detail: "error rate and latency shifted above baseline".to_string(),
                },
            ],
        }
    }

    #[test]
    fn generic_webhook_payload_keeps_text_only() {
        let verdict = sample_verdict();
        let payload = build_webhook_payload("https://example.test/hook", "plain alert", &verdict);
        let value = serde_json::to_value(payload).expect("serialize payload");

        assert_eq!(value["text"], "plain alert");
        assert!(value.get("blocks").is_none());
    }

    #[test]
    fn slack_webhook_payload_includes_timeline_blocks() {
        let verdict = sample_verdict();
        let payload = build_webhook_payload(
            "https://hooks.slack.com/services/T000/B000/XXX",
            "plain alert",
            &verdict,
        );
        let value = serde_json::to_value(payload).expect("serialize payload");
        let blocks = value["blocks"].as_array().expect("slack blocks");
        let serialized = serde_json::to_string(blocks).expect("blocks string");

        assert_eq!(value["text"], "plain alert");
        assert!(serialized.contains("WatchDog regression detected"));
        assert!(serialized.contains("Timeline"));
        assert!(serialized.contains("api: database timeout"));
    }
}
