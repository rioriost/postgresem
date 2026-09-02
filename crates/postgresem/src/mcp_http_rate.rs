use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    active: u32,
}

#[derive(Debug, Default)]
struct LimiterState {
    buckets: BTreeMap<String, Bucket>,
}

#[derive(Clone, Debug, Default)]
pub struct PrincipalLimiter {
    state: Arc<Mutex<LimiterState>>,
}

pub struct PrincipalPermit {
    authority_id: String,
    state: Arc<Mutex<LimiterState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    Rate,
    Concurrency,
    Unavailable,
}

impl PrincipalLimiter {
    pub fn acquire(
        &self,
        authority_id: &str,
        requests_per_minute: u32,
        burst: u32,
        max_concurrent: u32,
    ) -> Result<PrincipalPermit, LimitError> {
        let mut state = self.state.lock().map_err(|_| LimitError::Unavailable)?;
        let now = Instant::now();
        let bucket = state
            .buckets
            .entry(authority_id.to_owned())
            .or_insert_with(|| Bucket {
                tokens: f64::from(burst),
                last_refill: now,
                active: 0,
            });
        let elapsed = now.saturating_duration_since(bucket.last_refill);
        let refill_per_second = f64::from(requests_per_minute) / 60.0;
        bucket.tokens =
            (bucket.tokens + elapsed.as_secs_f64() * refill_per_second).min(f64::from(burst));
        bucket.last_refill = now;
        if bucket.active >= max_concurrent {
            return Err(LimitError::Concurrency);
        }
        if bucket.tokens < 1.0 {
            return Err(LimitError::Rate);
        }
        bucket.tokens -= 1.0;
        bucket.active += 1;
        Ok(PrincipalPermit {
            authority_id: authority_id.to_owned(),
            state: Arc::clone(&self.state),
        })
    }
}

impl Drop for PrincipalPermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(bucket) = state.buckets.get_mut(&self.authority_id) {
                bucket.active = bucket.active.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LimitError, PrincipalLimiter};

    #[test]
    fn limits_rate_and_concurrency_per_authority() {
        let limiter = PrincipalLimiter::default();
        let first = limiter
            .acquire("tenant-a", 1, 2, 1)
            .expect("first request is admitted");
        assert!(matches!(
            limiter.acquire("tenant-a", 1, 2, 1),
            Err(LimitError::Concurrency)
        ));
        assert!(limiter.acquire("tenant-b", 1, 2, 1).is_ok());
        drop(first);
        let second = limiter
            .acquire("tenant-a", 1, 2, 1)
            .expect("second burst token is admitted");
        drop(second);
        assert!(matches!(
            limiter.acquire("tenant-a", 1, 2, 1),
            Err(LimitError::Rate)
        ));
    }
}
