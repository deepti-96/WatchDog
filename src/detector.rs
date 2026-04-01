use crate::model::{BaselineSnapshot, MetricSample};

#[derive(Debug, Clone)]
pub struct Cusum {
    threshold: f64,
    drift: f64,
    sum: f64,
}

impl Cusum {
    pub fn new(threshold: f64, drift: f64) -> Self {
        Self {
            threshold,
            drift,
            sum: 0.0,
        }
    }

    pub fn update(&mut self, observed: f64, baseline: f64) -> bool {
        self.sum = (self.sum + observed - baseline - self.drift).max(0.0);
        self.sum > self.threshold
    }

    pub fn reset(&mut self) {
        self.sum = 0.0;
    }
}

#[derive(Debug, Clone)]
pub struct ChangeDetector {
    error_cusum: Cusum,
    latency_cusum: Cusum,
}

impl ChangeDetector {
    pub fn new() -> Self {
        Self {
            error_cusum: Cusum::new(0.08, 0.002),
            latency_cusum: Cusum::new(120.0, 5.0),
        }
    }

    pub fn detect(&mut self, sample: &MetricSample, baseline: &BaselineSnapshot) -> Option<String> {
        let error_shift = self.error_cusum.update(sample.error_rate, baseline.error_rate_mean);
        let latency_shift = self.latency_cusum.update(sample.p95_latency_ms, baseline.p95_latency_mean);

        match (error_shift, latency_shift) {
            (true, true) => Some("error rate and latency shifted above baseline".to_string()),
            (true, false) => Some("error rate shifted above baseline".to_string()),
            (false, true) => Some("latency shifted above baseline".to_string()),
            (false, false) => None,
        }
    }

    pub fn reset(&mut self) {
        self.error_cusum.reset();
        self.latency_cusum.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cusum_trips_after_sustained_shift() {
        let mut cusum = Cusum::new(2.0, 0.1);
        let baseline = 1.0;
        let series = [1.2, 1.4, 1.6, 1.9, 2.1];

        let mut triggered = false;
        for value in series {
            if cusum.update(value, baseline) {
                triggered = true;
                break;
            }
        }

        assert!(triggered);
    }
}
