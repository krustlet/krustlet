//! Provides backoff timing control for Kubernetes pod states
//! such as ImagePullBackoff and CrashLoopBackoff.
use std::time::Duration;

/// Determines how long to back off before performing a retry.
#[async_trait::async_trait]
pub trait BackoffStrategy: Send {
    /// Resets the strategy after a success.
    fn reset(&mut self);
    /// Gets how long to wait before retrying.
    fn next_duration(&mut self) -> Duration;
    /// Waits the prescribed amount of time (as per `next_duration`).
    async fn wait(&mut self) {
        tokio::time::sleep(self.next_duration()).await
    }
}

/// A `BackoffStrategy` in which the durations increase exponentially
/// until hitting a cap.
pub struct ExponentialBackoffStrategy {
    base_duration: Duration,
    cap: Duration,
    last_duration: Duration,
}

impl Default for ExponentialBackoffStrategy {
    /// Gets a backoff strategy that adheres to the Kubernetes defaults.
    fn default() -> Self {
        Self {
            base_duration: Duration::from_secs(10),
            cap: Duration::from_secs(300),
            last_duration: Duration::from_secs(0),
        }
    }
}

impl ExponentialBackoffStrategy {
    fn capped_next_duration(&self) -> Duration {
        let next_duration = if self.last_duration == Duration::from_secs(0) {
            self.base_duration
        } else {
            self.last_duration * 2
        };

        if next_duration > self.cap {
            self.cap
        } else {
            next_duration
        }
    }
}

impl BackoffStrategy for ExponentialBackoffStrategy {
    fn reset(&mut self) {
        self.last_duration = Duration::from_secs(0);
    }

    fn next_duration(&mut self) -> Duration {
        let next_duration = self.capped_next_duration();
        self.last_duration = next_duration;
        next_duration
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn first_backoff_is_10_seconds() {
        let mut backoff = ExponentialBackoffStrategy::default();
        assert_eq!(backoff.next_duration(), Duration::from_secs(10));
    }

    #[test]
    fn backoff_doubles_each_time() {
        let mut backoff = ExponentialBackoffStrategy::default();
        assert_eq!(backoff.next_duration(), Duration::from_secs(10));
        assert_eq!(backoff.next_duration(), Duration::from_secs(20));
        assert_eq!(backoff.next_duration(), Duration::from_secs(40));
        assert_eq!(backoff.next_duration(), Duration::from_secs(80));
    }

    #[test]
    fn after_reset_next_backoff_is_10_seconds() {
        let mut backoff = ExponentialBackoffStrategy::default();
        assert_eq!(backoff.next_duration(), Duration::from_secs(10));
        assert_eq!(backoff.next_duration(), Duration::from_secs(20));
        assert_eq!(backoff.next_duration(), Duration::from_secs(40));
        backoff.reset();
        assert_eq!(backoff.next_duration(), Duration::from_secs(10));
        assert_eq!(backoff.next_duration(), Duration::from_secs(20));
    }

    #[test]
    fn backoff_is_capped_at_5_minutes() {
        let mut backoff = ExponentialBackoffStrategy::default();
        assert_eq!(backoff.next_duration(), Duration::from_secs(10));
        assert_eq!(backoff.next_duration(), Duration::from_secs(20));
        assert_eq!(backoff.next_duration(), Duration::from_secs(40));
        assert_eq!(backoff.next_duration(), Duration::from_secs(80));
        assert_eq!(backoff.next_duration(), Duration::from_secs(160));
        assert_eq!(backoff.next_duration(), Duration::from_secs(300));
        assert_eq!(backoff.next_duration(), Duration::from_secs(300));
    }
}
