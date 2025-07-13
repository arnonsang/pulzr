use crate::cli::RampPattern;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RampUpConfig {
    pub duration: Duration,
    pub pattern: RampPattern,
    pub max_concurrent: usize,
}

impl RampUpConfig {
    pub fn new(duration: Duration, pattern: RampPattern, max_concurrent: usize) -> Self {
        Self {
            duration,
            pattern,
            max_concurrent,
        }
    }

    /// Calculate current concurrency level based on elapsed time
    pub fn current_concurrency(&self, start_time: Instant) -> usize {
        let elapsed = start_time.elapsed();

        // If ramp-up period is complete, return max concurrency
        if elapsed >= self.duration {
            return self.max_concurrent;
        }

        let progress = elapsed.as_secs_f64() / self.duration.as_secs_f64();
        let normalized_progress = progress.clamp(0.0, 1.0);

        let concurrency_ratio = match self.pattern {
            RampPattern::Linear => normalized_progress,
            RampPattern::Exponential => normalized_progress.powf(2.0),
            RampPattern::Step => {
                // Step pattern: 25%, 50%, 75%, 100% at quarters
                if normalized_progress < 0.25 {
                    0.25
                } else if normalized_progress < 0.50 {
                    0.50
                } else if normalized_progress < 0.75 {
                    0.75
                } else {
                    1.0
                }
            }
        };

        // Ensure at least 1 concurrent request
        let current = (self.max_concurrent as f64 * concurrency_ratio).ceil() as usize;
        current.max(1).min(self.max_concurrent)
    }

    /// Check if ramp-up is still active
    pub fn is_ramping(&self, start_time: Instant) -> bool {
        start_time.elapsed() < self.duration
    }

    /// Get description of the ramp-up pattern
    pub fn description(&self) -> String {
        match self.pattern {
            RampPattern::Linear => format!("Linear ramp-up over {}s", self.duration.as_secs()),
            RampPattern::Exponential => {
                format!("Exponential ramp-up over {}s", self.duration.as_secs())
            }
            RampPattern::Step => format!("Step ramp-up over {}s", self.duration.as_secs()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_ramp_up() {
        let config = RampUpConfig::new(Duration::from_secs(4), RampPattern::Linear, 10);

        // At start (0% progress) - should be at least 1
        let start = Instant::now();
        assert_eq!(config.current_concurrency(start), 1);

        // 50% progress - simulate by using a start time 2 seconds ago
        let mid_start = Instant::now() - Duration::from_secs(2);
        let mid_concurrency = config.current_concurrency(mid_start);
        // Linear ramp: 50% of 10 = 5, but with ceil() it might be 6
        assert!((5..=6).contains(&mid_concurrency));

        // 100% progress (past end) - simulate by using a start time 4+ seconds ago
        let end_start = Instant::now() - Duration::from_secs(5);
        assert_eq!(config.current_concurrency(end_start), 10);
    }

    #[test]
    fn test_step_ramp_up() {
        let config = RampUpConfig::new(Duration::from_secs(4), RampPattern::Step, 12); // Use 12 for cleaner math

        // Test step intervals - step pattern uses fixed percentages: 25%, 50%, 75%, 100%
        // At exactly 25% (0.25), the condition `< 0.25` is false, so it goes to 50%
        let start_25 = Instant::now() - Duration::from_secs(1); // 25% progress = 0.25
        let conc_25 = config.current_concurrency(start_25);
        assert_eq!(conc_25, 6); // At exactly 0.25, step pattern gives 50% = 6

        // Test with slightly less than 25% to get the 25% step
        let start_24 = Instant::now() - Duration::from_millis(900); // ~22.5% progress
        let conc_24 = config.current_concurrency(start_24);
        assert_eq!(conc_24, 3); // < 0.25 gives 25% = 3

        let start_50 = Instant::now() - Duration::from_secs(2); // 50% progress
        let conc_50 = config.current_concurrency(start_50);
        assert_eq!(conc_50, 9); // At exactly 0.50, gives 75% = 9

        let start_75 = Instant::now() - Duration::from_secs(3); // 75% progress
        let conc_75 = config.current_concurrency(start_75);
        assert_eq!(conc_75, 12); // At exactly 0.75, gives 100% = 12

        let start_100 = Instant::now() - Duration::from_secs(4); // 100% progress
        assert_eq!(config.current_concurrency(start_100), 12); // 100% of 12 = 12
    }

    #[test]
    fn test_ramp_up_completion() {
        let config = RampUpConfig::new(Duration::from_secs(2), RampPattern::Linear, 10);

        let start = Instant::now();
        assert!(config.is_ramping(start));

        let completed = Instant::now() - Duration::from_secs(3);
        assert!(!config.is_ramping(completed));
        assert_eq!(config.current_concurrency(completed), 10);
    }
}
