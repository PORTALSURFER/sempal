use std::time::Duration;

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;

use crate::http_client;

use super::Release;
use crate::updater::UpdateError;

const MAX_RELEASE_JSON_BYTES: usize = 2 * 1024 * 1024;
const GITHUB_RETRY_CONFIG: http_client::RetryConfig = http_client::RetryConfig {
    max_attempts: 4,
    base_delay: Duration::from_millis(200),
    max_delay: Duration::from_secs(2),
};

pub(super) fn fetch_releases(repo: &str) -> Result<Vec<Release>, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    get_json(&url)
}

pub(super) fn fetch_release_by_tag(repo: &str, tag: &str) -> Result<Release, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
    get_json(&url)
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, UpdateError> {
    get_json_with_retry(url, GITHUB_RETRY_CONFIG)
}

fn get_json_with_retry<T: for<'de> Deserialize<'de>>(
    url: &str,
    retry_config: http_client::RetryConfig,
) -> Result<T, UpdateError> {
    get_json_with_retry_from(retry_config, || {
        http_client::agent()
            .get(url)
            .set("User-Agent", "wavecrate")
            .set("Accept", "application/vnd.github+json")
            .call()
    })
}

fn get_json_with_retry_from<T: for<'de> Deserialize<'de>, F>(
    retry_config: http_client::RetryConfig,
    request: F,
) -> Result<T, UpdateError>
where
    F: FnMut() -> Result<ureq::Response, ureq::Error>,
{
    get_json_with_retry_from_with_sleep(retry_config, request, std::thread::sleep)
}

fn get_json_with_retry_from_with_sleep<T: for<'de> Deserialize<'de>, F, S>(
    retry_config: http_client::RetryConfig,
    request: F,
    sleep: S,
) -> Result<T, UpdateError>
where
    F: FnMut() -> Result<ureq::Response, ureq::Error>,
    S: FnMut(Duration),
{
    let response =
        http_client::retry_with_policy_using(retry_config, request, github_retry_decision, sleep)
            .map_err(map_github_error)?;
    let bytes = http_client::read_response_bytes(response, MAX_RELEASE_JSON_BYTES)
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn github_retry_decision(err: &ureq::Error) -> http_client::RetryDecision {
    match err {
        ureq::Error::Transport(_) => http_client::RetryDecision::Retry,
        ureq::Error::Status(429, _) => retry_after_delay(err)
            .map(http_client::RetryDecision::RetryAfter)
            .unwrap_or(http_client::RetryDecision::Retry),
        ureq::Error::Status(code, _) if (500..=599).contains(code) => {
            http_client::RetryDecision::Retry
        }
        ureq::Error::Status(_, _) => http_client::RetryDecision::Stop,
    }
}

fn retry_after_delay(err: &ureq::Error) -> Option<Duration> {
    let ureq::Error::Status(_, response) = err else {
        return None;
    };
    let header = response.header("Retry-After")?;
    parse_retry_after(header)
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let Ok(retry_at) = OffsetDateTime::parse(trimmed, &Rfc2822) else {
        return None;
    };
    let now = OffsetDateTime::now_utc();
    let delta = retry_at - now;
    if !delta.is_positive() {
        return Some(Duration::from_secs(0));
    }
    let secs = u64::try_from(delta.whole_seconds()).ok()?;
    let nanos = u32::try_from(delta.subsec_nanoseconds()).ok()?;
    Some(Duration::new(secs, nanos))
}

fn map_github_error(err: ureq::Error) -> UpdateError {
    match err {
        ureq::Error::Status(code, _) => UpdateError::Http(format!("HTTP {code}")),
        ureq::Error::Transport(err) => UpdateError::Http(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn retries_on_transient_github_errors() {
        let body = r#"{"ok": true}"#;
        let responses = [
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n"
                .to_string(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            ),
        ];
        let attempts = AtomicUsize::new(0);
        let config = http_client::RetryConfig {
            max_attempts: 4,
            base_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
        };

        let mut index = 0usize;
        let value: serde_json::Value = get_json_with_retry_from_with_sleep(
            config,
            || {
                let current =
                    response_from_str(responses.get(index).expect("response sequence exhausted"));
                attempts.fetch_add(1, Ordering::SeqCst);
                index += 1;
                if index < 3 {
                    Err(ureq::Error::Status(current.status(), current))
                } else {
                    Ok(current)
                }
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(value["ok"].as_bool(), Some(true));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn retry_after_delay_is_supplied_to_shared_retry_executor() {
        let responses = [
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n"
                .to_string(),
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}".to_string(),
        ];
        let attempts = AtomicUsize::new(0);
        let mut delays = Vec::new();
        let config = http_client::RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_secs(2),
        };
        let mut index = 0usize;

        let value: serde_json::Value = get_json_with_retry_from_with_sleep(
            config,
            || {
                let current =
                    response_from_str(responses.get(index).expect("response sequence exhausted"));
                attempts.fetch_add(1, Ordering::SeqCst);
                index += 1;
                if index == 1 {
                    Err(ureq::Error::Status(current.status(), current))
                } else {
                    Ok(current)
                }
            },
            |delay| delays.push(delay),
        )
        .unwrap();

        assert_eq!(value["ok"].as_bool(), Some(true));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(delays, vec![Duration::from_secs(2)]);
    }

    fn response_from_str(raw: &str) -> ureq::Response {
        raw.parse().expect("valid test response")
    }
}
