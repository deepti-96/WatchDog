use crate::engine::WatchdogEngine;
use crate::model::{DeployEvent, MetricSample};
use chrono::{Duration, Utc};

#[derive(Debug, Clone)]
pub struct BenchmarkSummary {
    pub trials: usize,
    pub healthy_false_positives: usize,
    pub bad_detected: usize,
    pub bad_missed: usize,
    pub average_detection_secs: f64,
    pub best_detection_secs: i64,
    pub worst_detection_secs: i64,
}

pub fn run(trials: usize, monitoring_window_secs: u64) -> BenchmarkSummary {
    let mut healthy_false_positives = 0usize;
    let mut bad_detected = 0usize;
    let mut bad_missed = 0usize;
    let mut latencies = Vec::new();

    for trial in 0..trials {
        if run_scenario(trial, false, monitoring_window_secs).is_some() {
            healthy_false_positives += 1;
        }

        match run_scenario(trial, true, monitoring_window_secs) {
            Some(latency) => {
                bad_detected += 1;
                latencies.push(latency);
            }
            None => bad_missed += 1,
        }
    }

    let average_detection_secs = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<i64>() as f64 / latencies.len() as f64
    };

    let best_detection_secs = latencies.iter().copied().min().unwrap_or(0);
    let worst_detection_secs = latencies.iter().copied().max().unwrap_or(0);

    BenchmarkSummary {
        trials,
        healthy_false_positives,
        bad_detected,
        bad_missed,
        average_detection_secs,
        best_detection_secs,
        worst_detection_secs,
    }
}

fn run_scenario(seed: usize, bad_deploy: bool, monitoring_window_secs: u64) -> Option<i64> {
    let mut engine = WatchdogEngine::new(120, monitoring_window_secs as i64);
    let start = Utc::now() + Duration::seconds(seed as i64 * 1000);

    for i in 0..30 {
        let jitter = ((seed + i as usize) % 4) as f64;
        engine.ingest_metric(MetricSample {
            timestamp: start + Duration::seconds(i),
            error_rate: 0.009 + jitter * 0.001,
            p95_latency_ms: 112.0 + jitter * 4.0,
            request_rate: 390.0 + jitter * 3.0,
        });
    }

    let deploy_time = start + Duration::seconds(31);
    let armed = engine.mark_deploy(DeployEvent {
        timestamp: deploy_time,
        deploy_id: format!("trial-{seed}"),
        environment: "benchmark".to_string(),
    });
    if !armed {
        return None;
    }

    for i in 32..60 {
        let drift = ((seed + i as usize) % 3) as f64;
        let degraded = bad_deploy && i >= 35;
        let verdict = engine.ingest_metric(MetricSample {
            timestamp: start + Duration::seconds(i),
            error_rate: if degraded { 0.085 + drift * 0.012 } else { 0.010 + drift * 0.001 },
            p95_latency_ms: if degraded { 245.0 + drift * 18.0 } else { 118.0 + drift * 4.0 },
            request_rate: 405.0 + drift * 2.0,
        });

        if let Some(verdict) = verdict {
            return Some(verdict.seconds_after_deploy);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_catches_bad_deploys_without_healthy_noise() {
        let summary = run(20, 300);
        assert_eq!(summary.healthy_false_positives, 0);
        assert_eq!(summary.bad_missed, 0);
        assert!(summary.bad_detected >= 20);
    }
}
