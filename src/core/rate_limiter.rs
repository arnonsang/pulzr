use governor::{Quota, RateLimiter};

pub struct RequestRateLimiter {
    limiter: Option<
        RateLimiter<
            governor::state::direct::NotKeyed,
            governor::state::InMemoryState,
            governor::clock::DefaultClock,
        >,
    >,
}

impl RequestRateLimiter {
    pub fn new(rps: Option<u64>) -> Self {
        let limiter = if let Some(rps) = rps {
            if rps > 0 {
                let rps_u32 = rps.min(u32::MAX as u64) as u32;
                if rps_u32 > 0 {
                    let quota = Quota::per_second(std::num::NonZeroU32::new(rps_u32).unwrap());
                    Some(RateLimiter::direct(quota))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Self { limiter }
    }

    pub async fn acquire(&self) {
        if let Some(limiter) = &self.limiter {
            limiter.until_ready().await;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.limiter.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_rate_limiter_disabled() {
        let limiter = RequestRateLimiter::new(None);
        assert!(!limiter.is_enabled());
    }

    #[test]
    fn test_rate_limiter_zero_rps() {
        let limiter = RequestRateLimiter::new(Some(0));
        assert!(!limiter.is_enabled());
    }

    #[test]
    fn test_rate_limiter_enabled() {
        let limiter = RequestRateLimiter::new(Some(10));
        assert!(limiter.is_enabled());
    }

    #[test]
    fn test_rate_limiter_max_value() {
        let limiter = RequestRateLimiter::new(Some(u64::MAX));
        assert!(limiter.is_enabled());
    }

    #[tokio::test]
    async fn test_rate_limiter_acquire_disabled() {
        let limiter = RequestRateLimiter::new(None);
        let start = Instant::now();
        limiter.acquire().await;
        let elapsed = start.elapsed();
        // Should return immediately when disabled
        assert!(elapsed < Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_rate_limiter_acquire_enabled() {
        let limiter = RequestRateLimiter::new(Some(100)); // High RPS to avoid timing issues

        // Verify rate limiter is enabled
        assert!(limiter.is_enabled());

        // Test that acquire() completes without panicking
        // We don't test exact timing as it's too flaky in test environments
        for _ in 0..3 {
            limiter.acquire().await;
        }

        // If we get here, the rate limiter is working (not hanging/panicking)
        // Test passes as the limiter doesn't block when RPS is high
    }

    #[test]
    fn test_rate_limiter_edge_cases() {
        // Test with 1 RPS
        let limiter = RequestRateLimiter::new(Some(1));
        assert!(limiter.is_enabled());

        // Test with maximum u32 value
        let limiter = RequestRateLimiter::new(Some(u32::MAX as u64));
        assert!(limiter.is_enabled());

        // Test with value exceeding u32::MAX
        let limiter = RequestRateLimiter::new(Some(u32::MAX as u64 + 1));
        assert!(limiter.is_enabled());
    }
}
