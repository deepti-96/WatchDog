use crate::model::RegressionVerdict;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct WebhookPayload<'a> {
    text: &'a str,
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

pub async fn send_webhook(webhook_url: &str, body: &str) -> Result<()> {
    let client = reqwest::Client::new();
    client
        .post(webhook_url)
        .json(&WebhookPayload { text: body })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
