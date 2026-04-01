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
        let latency_sum: f64 = self.samples.iter().map(|sample| sample.p95_latency_ms).sum();

        Some(BaselineSnapshot {
            error_rate_mean: error_sum / count as f64,
            p95_latency_mean: latency_sum / count as f64,
            sample_count: count,
        })
    }
}
