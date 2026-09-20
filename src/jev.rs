mod dto;

use std::time::{Duration, SystemTime};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use reqwest::{Client, StatusCode, Url};
use thiserror::Error;
use tokio::time::{Instant, sleep, timeout_at};

use crate::decision::{DecisionRequest, DecisionResult};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MAX_ATTEMPTS: usize = 3;
const MAX_SUCCESS_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JevError {
    #[error("Jev authentication failed")]
    Authentication,
    #[error("Jev rejected the request")]
    InvalidRequest,
    #[error("Jev rate limit reached")]
    RateLimited,
    #[error("Jev is overloaded")]
    Overloaded,
    #[error("Jev request timed out")]
    Timeout,
    #[error("Jev returned an invalid response")]
    InvalidResponse,
    #[error("Jev transport failed")]
    Transport,
    #[error("Jev client configuration is invalid")]
    Configuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JevClientTimings {
    pub connect_timeout: Duration,
    pub attempt_timeout: Duration,
    pub total_timeout: Duration,
    pub retry_delays: [Duration; 2],
}

impl Default for JevClientTimings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            attempt_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(30),
            retry_delays: [Duration::from_millis(500), Duration::from_secs(1)],
        }
    }
}

#[derive(Clone)]
pub struct JevClient {
    http: Client,
    endpoint: Url,
    authorization: HeaderValue,
    model: String,
    timings: JevClientTimings,
}

impl JevClient {
    pub fn new(api_key: &str, model: &str) -> Result<Self, JevError> {
        Self::with_endpoint_and_timings(api_key, model, ENDPOINT, JevClientTimings::default())
    }

    #[doc(hidden)]
    pub fn with_endpoint_and_timings(
        api_key: &str,
        model: &str,
        endpoint: &str,
        timings: JevClientTimings,
    ) -> Result<Self, JevError> {
        if api_key.is_empty()
            || model.is_empty()
            || timings.connect_timeout.is_zero()
            || timings.attempt_timeout.is_zero()
            || timings.total_timeout.is_zero()
        {
            return Err(JevError::Configuration);
        }

        let endpoint = Url::parse(endpoint).map_err(|_| JevError::Configuration)?;
        validate_endpoint(&endpoint)?;
        let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|_| JevError::Configuration)?;
        authorization.set_sensitive(true);
        let http = Client::builder()
            .connect_timeout(timings.connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| JevError::Configuration)?;

        Ok(Self {
            http,
            endpoint,
            authorization,
            model: model.to_owned(),
            timings,
        })
    }

    pub async fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, JevError> {
        let deadline = Instant::now() + self.timings.total_timeout;
        match timeout_at(deadline, self.decide_until(request, deadline)).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!("Jev request reached its overall deadline");
                Err(JevError::Timeout)
            }
        }
    }

    async fn decide_until(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionResult, JevError> {
        let payload = dto::serialize_request(request, &self.model)?;

        for attempt in 0..MAX_ATTEMPTS {
            let now = Instant::now();
            if now >= deadline {
                return Err(JevError::Timeout);
            }
            let attempt_deadline = (now + self.timings.attempt_timeout).min(deadline);
            let response = match timeout_at(attempt_deadline, self.send_once(payload.clone())).await
            {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    tracing::warn!(attempt = attempt + 1, "Jev HTTP attempt failed");
                    return Err(error);
                }
                Err(_) => {
                    tracing::warn!(attempt = attempt + 1, "Jev HTTP attempt timed out");
                    return Err(JevError::Timeout);
                }
            };
            let status = response.status();

            if status.is_success() {
                let body = match timeout_at(attempt_deadline, read_success_body(response)).await {
                    Ok(Ok(body)) => body,
                    Ok(Err(error)) => {
                        tracing::warn!(attempt = attempt + 1, "Jev response body failed");
                        return Err(error);
                    }
                    Err(_) => {
                        tracing::warn!(attempt = attempt + 1, "Jev response body timed out");
                        return Err(JevError::Timeout);
                    }
                };
                let result = dto::parse_response(&body, request);
                if result.is_err() {
                    tracing::warn!(attempt = attempt + 1, "Jev response contract was invalid");
                }
                return result;
            }

            let error = classify_status(status);
            tracing::warn!(
                attempt = attempt + 1,
                status = status.as_u16(),
                "Jev request returned an HTTP error"
            );
            if !matches!(status.as_u16(), 429 | 529) || attempt + 1 == MAX_ATTEMPTS {
                return Err(error);
            }

            let delay = retry_delay(
                response.headers().get(RETRY_AFTER),
                SystemTime::now(),
                self.timings.retry_delays[attempt],
            );
            drop(response);
            let remaining = deadline.saturating_duration_since(Instant::now());
            if delay >= remaining {
                return Err(error);
            }
            sleep(delay).await;
        }

        Err(JevError::Transport)
    }

    async fn send_once(&self, payload: Vec<u8>) -> Result<reqwest::Response, JevError> {
        self.http
            .post(self.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .map_err(classify_reqwest_error)
    }
}

fn validate_endpoint(endpoint: &Url) -> Result<(), JevError> {
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.host_str().is_none()
    {
        return Err(JevError::Configuration);
    }
    match endpoint.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = endpoint
                .host()
                .map(|host| host.to_string())
                .ok_or(JevError::Configuration)?;
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
                .filter(std::net::IpAddr::is_loopback)
                .map(|_| ())
                .ok_or(JevError::Configuration)
        }
        _ => Err(JevError::Configuration),
    }
}

fn classify_status(status: StatusCode) -> JevError {
    match status.as_u16() {
        401 | 403 => JevError::Authentication,
        400 | 422 => JevError::InvalidRequest,
        429 => JevError::RateLimited,
        529 => JevError::Overloaded,
        _ => JevError::Transport,
    }
}

async fn read_success_body(mut response: reqwest::Response) -> Result<Vec<u8>, JevError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SUCCESS_BODY as u64)
    {
        return Err(JevError::InvalidResponse);
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_reqwest_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_SUCCESS_BODY {
            return Err(JevError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_reqwest_error(error: reqwest::Error) -> JevError {
    if error.is_timeout() {
        JevError::Timeout
    } else {
        JevError::Transport
    }
}

fn retry_after_delay(value: Option<&HeaderValue>, now: SystemTime) -> Option<Duration> {
    let value = value?.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(date.duration_since(now).unwrap_or(Duration::ZERO))
}

fn retry_delay(value: Option<&HeaderValue>, now: SystemTime, baseline: Duration) -> Duration {
    retry_after_delay(value, now)
        .unwrap_or_default()
        .max(baseline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_retry_delay_from_seconds_dates_and_invalid_values() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let future = httpdate::fmt_http_date(now + Duration::from_secs(17));
        let past = httpdate::fmt_http_date(now - Duration::from_secs(17));
        let baseline = Duration::from_secs(5);

        assert_eq!(
            retry_delay(Some(&HeaderValue::from_static("12")), now, baseline),
            Duration::from_secs(12)
        );
        assert_eq!(
            retry_delay(
                Some(&HeaderValue::from_str(&future).unwrap()),
                now,
                baseline
            ),
            Duration::from_secs(17)
        );
        assert_eq!(
            retry_delay(Some(&HeaderValue::from_str(&past).unwrap()), now, baseline),
            baseline
        );
        assert_eq!(
            retry_delay(Some(&HeaderValue::from_static("invalid")), now, baseline),
            baseline
        );
        assert_eq!(retry_delay(None, now, baseline), baseline);
    }
}
