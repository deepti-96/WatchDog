use crate::buffer::RingBuffer;
use crate::detector::ChangeDetector;
use crate::model::{BaselineSnapshot, DeployEvent, MetricSample, RegressionVerdict};
use chrono::Duration;

#[derive(Debug, Clone)]
struct ActiveDeploy {
    event: DeployEvent,
    baseline: BaselineSnapshot,
}

#[derive(Debug)]
pub struct WatchdogEngine {
    ring: RingBuffer,
    detector: ChangeDetector,
    monitoring_window: Duration,
    active_deploy: Option<ActiveDeploy>,
}

impl WatchdogEngine {
    pub fn new(baseline_capacity: usize, monitoring_window_secs: i64) -> Self {
        Self {
            ring: RingBuffer::new(baseline_capacity),
            detector: ChangeDetector::new(),
            monitoring_window: Duration::seconds(monitoring_window_secs),
            active_deploy: None,
        }
    }

    pub fn ingest_metric(&mut self, sample: MetricSample) -> Option<RegressionVerdict> {
        if let Some(active) = &self.active_deploy {
            let within_window = sample.timestamp <= active.event.timestamp + self.monitoring_window;
            if within_window {
                if let Some(reason) = self.detector.detect(&sample, &active.baseline) {
                    let verdict = RegressionVerdict {
                        deploy_id: active.event.deploy_id.clone(),
                        detected_at: sample.timestamp,
                        seconds_after_deploy: (sample.timestamp - active.event.timestamp).num_seconds(),
                        error_rate_delta: sample.error_rate - active.baseline.error_rate_mean,
                        latency_delta_ms: sample.p95_latency_ms - active.baseline.p95_latency_mean,
                        reason,
                    };
                    self.active_deploy = None;
                    self.detector.reset();
                    self.ring.push(sample);
                    return Some(verdict);
                }
            } else {
                self.active_deploy = None;
                self.detector.reset();
            }
        }

        self.ring.push(sample);
        None
    }

    pub fn mark_deploy(&mut self, event: DeployEvent) -> bool {
        let Some(baseline) = self.ring.baseline() else {
            return false;
        };

        if baseline.sample_count < 10 {
            return false;
        }

        self.detector.reset();
        self.active_deploy = Some(ActiveDeploy { event, baseline });
        true
    }

    pub fn baseline_size(&self) -> usize {
        self.ring.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricSample;
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
}
