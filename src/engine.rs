use crate::buffer::RingBuffer;
use crate::detector::ChangeDetector;
use crate::logs::{extract_error_signature, ErrorRingBuffer};
use crate::model::{
    BaselineSnapshot, DeployEvent, IncidentMetricComparison, IncidentTimelineEvent, LogEvent,
    MetricSample, RegressionVerdict,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct ErrorObservation {
    count: usize,
    first_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct ActiveDeploy {
    event: DeployEvent,
    baseline: BaselineSnapshot,
    baseline_errors: HashMap<String, usize>,
    post_errors: HashMap<String, ErrorObservation>,
}

#[derive(Debug)]
pub struct WatchdogEngine {
    ring: RingBuffer,
    detector: ChangeDetector,
    error_ring: ErrorRingBuffer,
    monitoring_window: Duration,
    active_deploy: Option<ActiveDeploy>,
}

impl WatchdogEngine {
    pub fn new(baseline_capacity: usize, monitoring_window_secs: i64) -> Self {
        Self {
            ring: RingBuffer::new(baseline_capacity),
            detector: ChangeDetector::new(),
            error_ring: ErrorRingBuffer::new(baseline_capacity),
            monitoring_window: Duration::seconds(monitoring_window_secs),
            active_deploy: None,
        }
    }

    pub fn ingest_metric(&mut self, sample: MetricSample) -> Option<RegressionVerdict> {
        if let Some(active) = &self.active_deploy {
            let within_window = sample.timestamp <= active.event.timestamp + self.monitoring_window;
            if within_window {
                if let Some(reason) = self.detector.detect(&sample, &active.baseline) {
                    let verdict = self.build_verdict(sample, reason);
                    self.detector.reset();
                    return verdict;
                }
            } else {
                self.active_deploy = None;
                self.detector.reset();
            }
        }

        self.ring.push(sample);
        None
    }

    pub fn ingest_log(&mut self, event: LogEvent) {
        let within_window = self
            .active_deploy
            .as_ref()
            .map(|active| event.timestamp <= active.event.timestamp + self.monitoring_window)
            .unwrap_or(false);

        if let Some(signature) = extract_error_signature(&event) {
            if within_window {
                if let Some(active) = &mut self.active_deploy {
                    let observation = active.post_errors.entry(signature.clone()).or_insert(ErrorObservation {
                        count: 0,
                        first_seen_at: event.timestamp,
                    });
                    observation.count += 1;
                }
            }
            self.error_ring.push(signature);
        }

        if let Some(active) = &self.active_deploy {
            if event.timestamp > active.event.timestamp + self.monitoring_window {
                self.active_deploy = None;
                self.detector.reset();
            }
        }
    }

    pub fn mark_deploy(&mut self, event: DeployEvent) -> bool {
        let Some(baseline) = self.ring.baseline() else {
            return false;
        };

        if baseline.sample_count < 10 {
            return false;
        }

        self.detector.reset();
        self.active_deploy = Some(ActiveDeploy {
            event,
            baseline,
            baseline_errors: self.error_ring.snapshot_counts(),
            post_errors: HashMap::new(),
        });
        true
    }

    pub fn baseline_size(&self) -> usize {
        self.ring.len()
    }

    fn build_verdict(&mut self, sample: MetricSample, reason: String) -> Option<RegressionVerdict> {
        let active = self.active_deploy.take()?;
        let (top_error_signature, top_error_count, top_error_is_new, first_error_at) = dominant_error_summary(&active);

        let mut timeline = vec![
            IncidentTimelineEvent {
                label: "Deploy started".to_string(),
                timestamp: active.event.timestamp,
                detail: format!("{} deployed to {}", active.event.deploy_id, active.event.environment),
            },
        ];

        if let (Some(signature), Some(first_seen_at)) = (&top_error_signature, first_error_at) {
            timeline.push(IncidentTimelineEvent {
                label: "First dominant error".to_string(),
                timestamp: first_seen_at,
                detail: format!("{}", signature),
            });
        }

        timeline.push(IncidentTimelineEvent {
            label: "Regression detected".to_string(),
            timestamp: sample.timestamp,
            detail: format!("{}", reason),
        });

        let verdict = RegressionVerdict {
            deploy_id: active.event.deploy_id.clone(),
            environment: active.event.environment.clone(),
            deploy_timestamp: active.event.timestamp,
            detected_at: sample.timestamp,
            seconds_after_deploy: (sample.timestamp - active.event.timestamp).num_seconds(),
            error_rate_delta: sample.error_rate - active.baseline.error_rate_mean,
            latency_delta_ms: sample.p95_latency_ms - active.baseline.p95_latency_mean,
            reason,
            top_error_signature,
            top_error_count,
            top_error_is_new,
            comparison: IncidentMetricComparison {
                baseline_error_rate: active.baseline.error_rate_mean,
                detected_error_rate: sample.error_rate,
                baseline_latency_ms: active.baseline.p95_latency_mean,
                detected_latency_ms: sample.p95_latency_ms,
                request_rate_at_detection: sample.request_rate,
            },
            timeline,
        };

        self.ring.push(sample);
        Some(verdict)
    }
}

fn dominant_error_summary(active: &ActiveDeploy) -> (Option<String>, usize, bool, Option<DateTime<Utc>>) {
    let Some((signature, observation)) = active.post_errors.iter().max_by(|left, right| {
        left.1
            .count
            .cmp(&right.1.count)
            .then_with(|| left.0.cmp(right.0))
    }) else {
        return (None, 0, false, None);
    };

    let top_error_is_new = !active.baseline_errors.contains_key(signature);
    (
        Some(signature.clone()),
        observation.count,
        top_error_is_new,
        Some(observation.first_seen_at),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn regression_is_attributed_to_recent_deploy() {
        let mut engine = WatchdogEngine::new(64, 300);
        let start = Utc::now();

        for i in 0..20 {
            engine.ingest_metric(MetricSample {
                timestamp: start + Duration::seconds(i),
                error_rate: 0.01,
                p95_latency_ms: 110.0,
                request_rate: 400.0,
            });
        }

        let armed = engine.mark_deploy(DeployEvent {
            timestamp: start + Duration::seconds(21),
            deploy_id: "v1.2.3".to_string(),
            environment: "test".to_string(),
        });
        assert!(armed);

        let verdict = engine.ingest_metric(MetricSample {
            timestamp: start + Duration::seconds(24),
            error_rate: 0.12,
            p95_latency_ms: 280.0,
            request_rate: 405.0,
        });

        assert!(verdict.is_some());
        assert_eq!(verdict.unwrap().deploy_id, "v1.2.3");
    }

    #[test]
    fn regression_includes_dominant_post_deploy_error_signature() {
        let mut engine = WatchdogEngine::new(64, 300);
        let start = Utc::now();

        for i in 0..20 {
            engine.ingest_metric(MetricSample {
                timestamp: start + Duration::seconds(i),
                error_rate: 0.01,
                p95_latency_ms: 110.0,
                request_rate: 400.0,
            });
        }

        let armed = engine.mark_deploy(DeployEvent {
            timestamp: start + Duration::seconds(21),
            deploy_id: "v2.0.0".to_string(),
            environment: "test".to_string(),
        });
        assert!(armed);

        for i in 22..25 {
            engine.ingest_log(LogEvent {
                timestamp: start + Duration::seconds(i),
                level: "ERROR".to_string(),
                service: "api".to_string(),
                message: "Database timeout for user 123 request 8f91ab22".to_string(),
            });
        }

        let verdict = engine.ingest_metric(MetricSample {
            timestamp: start + Duration::seconds(25),
            error_rate: 0.13,
            p95_latency_ms: 290.0,
            request_rate: 405.0,
        });

        let verdict = verdict.expect("expected regression verdict");
        assert!(verdict.top_error_signature.is_some());
        assert_eq!(verdict.environment, "test");
        assert_eq!(verdict.top_error_count, 3);
        assert!(verdict.top_error_is_new);
        assert_eq!(verdict.timeline.len(), 3);
        assert!(verdict.comparison.detected_error_rate > verdict.comparison.baseline_error_rate);
    }
}
