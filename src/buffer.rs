use crate::model::{BaselineSnapshot, MetricSample};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct RingBuffer {
    cap: usize,
    samples: VecDeque<MetricSample>,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            samples: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, sample: MetricSample) {
        if self.cap == 0 {
            return;
        }

        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn baseline(&self) -> Option<BaselineSnapshot> {
        if self.samples.is_empty() {
            return None;
        }

        let count = self.samples.len();
        let error_sum: f64 = self.samples.iter().map(|sample| sample.error_rate).sum();
        let latency_sum: f64 = self
            .samples
            .iter()
            .map(|sample| sample.p95_latency_ms)
            .sum();

        Some(BaselineSnapshot {
            error_rate_mean: error_sum / count as f64,
            p95_latency_mean: latency_sum / count as f64,
            sample_count: count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample() -> MetricSample {
        MetricSample {
            timestamp: Utc::now(),
            error_rate: 0.01,
            p95_latency_ms: 120.0,
            request_rate: 400.0,
        }
    }

    #[test]
    fn zero_capacity_buffer_does_not_store_samples() {
        let mut buffer = RingBuffer::new(0);

        buffer.push(sample());

        assert_eq!(buffer.len(), 0);
        assert!(buffer.baseline().is_none());
    }

    #[test]
    fn buffer_keeps_only_the_latest_samples() {
        let mut buffer = RingBuffer::new(2);

        buffer.push(MetricSample {
            error_rate: 0.01,
            ..sample()
        });
        buffer.push(MetricSample {
            error_rate: 0.03,
            ..sample()
        });
        buffer.push(MetricSample {
            error_rate: 0.05,
            ..sample()
        });

        let baseline = buffer.baseline().expect("baseline");
        assert_eq!(buffer.len(), 2);
        assert_eq!(baseline.sample_count, 2);
        assert!((baseline.error_rate_mean - 0.04).abs() < f64::EPSILON);
    }
}
