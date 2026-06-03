use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::inventory::limits::MAX_HTTP_BODY_BYTES;
use crate::inventory::redaction::{redact_error, redact_json};

#[derive(Debug, Clone)]
pub struct HttpProbe {
    client: reqwest::Client,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct HttpJsonResult {
    pub status: u16,
    pub body: Value,
    pub elapsed_ms: u128,
    pub truncated: bool,
}

impl HttpProbe {
    pub fn new(timeout: Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()?;
        Ok(Self { client, timeout })
    }

    pub async fn get_json(&self, url: &str, headers: HeaderMap) -> Result<HttpJsonResult> {
        let start = Instant::now();
        let response = self
            .client
            .get(url)
            .headers(headers)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(redacted_reqwest_error)?;
        self.read_json(response, start).await
    }

    pub async fn post_json(
        &self,
        url: &str,
        headers: HeaderMap,
        body: Value,
    ) -> Result<HttpJsonResult> {
        let start = Instant::now();
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(redacted_reqwest_error)?;
        self.read_json(response, start).await
    }

    async fn read_json(
        &self,
        response: reqwest::Response,
        start: Instant,
    ) -> Result<HttpJsonResult> {
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(redacted_reqwest_error)?;
            let remaining = MAX_HTTP_BODY_BYTES.saturating_add(1) - bytes.len();
            if chunk.len() > remaining {
                bytes.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_HTTP_BODY_BYTES {
                truncated = true;
                break;
            }
        }
        let slice = if truncated {
            &bytes[..MAX_HTTP_BODY_BYTES]
        } else {
            bytes.as_slice()
        };
        let body = serde_json::from_slice(slice)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(slice).to_string()));
        Ok(HttpJsonResult {
            status,
            body: redact_json(&body),
            elapsed_ms: start.elapsed().as_millis(),
            truncated,
        })
    }
}

pub fn api_key_header(name: &'static str, value: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(name, HeaderValue::from_str(value)?);
    Ok(headers)
}

fn redacted_reqwest_error(error: reqwest::Error) -> anyhow::Error {
    let (message, _) = redact_error(error.to_string());
    anyhow!(message)
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
