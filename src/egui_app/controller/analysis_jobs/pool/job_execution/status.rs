use std::thread::sleep;
use std::time::Duration;

pub(super) fn update_job_status_with_retry<F>(update: F)
where
    F: FnMut() -> Result<(), String>,
{
    let _ = update_job_status_with_retry_inner(update, 5, Duration::from_millis(50));
}

fn update_job_status_with_retry_inner<F>(mut update: F, retries: usize, delay: Duration) -> bool
where
    F: FnMut() -> Result<(), String>,
{
    for attempt in 0..retries {
        match update() {
            Ok(()) => return true,
            Err(_) if attempt + 1 < retries => {
                if !delay.is_zero() {
                    sleep(delay);
                }
            }
            Err(err) => {
                tracing::warn!("Failed to update analysis job status: {err}");
                return false;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::update_job_status_with_retry_inner;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn update_job_status_retries_until_success() {
        let attempts = AtomicUsize::new(0);
        let ok = update_job_status_with_retry_inner(
            || {
                let attempt = attempts.fetch_add(1, Ordering::Relaxed) + 1;
                if attempt < 3 {
                    Err("fail".to_string())
                } else {
                    Ok(())
                }
            },
            5,
            Duration::from_millis(0),
        );
        assert!(ok);
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn update_job_status_stops_after_retries() {
        let attempts = AtomicUsize::new(0);
        let ok = update_job_status_with_retry_inner(
            || {
                attempts.fetch_add(1, Ordering::Relaxed);
                Err("fail".to_string())
            },
            4,
            Duration::from_millis(0),
        );
        assert!(!ok);
        assert_eq!(attempts.load(Ordering::Relaxed), 4);
    }
}
