use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;
use serde_json::{Value as JsonValue, json};

use crate::{LambdaContextSeed, LambdaRuntime};

const NEXT_PATH: &str = "/2018-06-01/runtime/invocation/next";

pub fn run_runtime_api_loop(task_root: &Path, runtime_api: &str) -> Result<()> {
    let runtime = LambdaRuntime::discover(task_root)?;
    let client = Client::new();
    let endpoint = RuntimeEndpoint::new(runtime_api);

    loop {
        let invocation = next_invocation(&client, &endpoint)?;
        let request_id = invocation.request_id.clone();
        let response = runtime.invoke_json_with_context(invocation.event, invocation.context);
        match response {
            Ok(response) => post_invocation_response(&client, &endpoint, &request_id, &response)?,
            Err(error) => post_invocation_error(&client, &endpoint, &request_id, &error)?,
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeEndpoint {
    base_url: String,
}

impl RuntimeEndpoint {
    fn new(runtime_api: &str) -> Self {
        let base = if runtime_api.starts_with("http://") || runtime_api.starts_with("https://") {
            runtime_api.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", runtime_api.trim_end_matches('/'))
        };
        Self { base_url: base }
    }

    fn next_url(&self) -> String {
        format!("{}{}", self.base_url, NEXT_PATH)
    }

    fn response_url(&self, request_id: &str) -> String {
        format!(
            "{}/2018-06-01/runtime/invocation/{}/response",
            self.base_url, request_id
        )
    }

    fn error_url(&self, request_id: &str) -> String {
        format!(
            "{}/2018-06-01/runtime/invocation/{}/error",
            self.base_url, request_id
        )
    }
}

#[derive(Debug)]
struct Invocation {
    request_id: String,
    context: LambdaContextSeed,
    event: JsonValue,
}

fn next_invocation(client: &Client, endpoint: &RuntimeEndpoint) -> Result<Invocation> {
    let response = client
        .get(endpoint.next_url())
        .send()
        .context("failed to fetch next Lambda invocation")?;
    if !response.status().is_success() {
        bail!(
            "Lambda Runtime API next invocation failed: {}",
            response.status()
        );
    }

    let headers = response.headers().clone();
    let event = response
        .json::<JsonValue>()
        .context("failed to decode Lambda invocation event JSON")?;
    let request_id = header_string(&headers, "lambda-runtime-aws-request-id")
        .ok_or_else(|| anyhow!("Lambda invocation missing request id header"))?;
    let context = context_from_headers(&headers, &request_id);

    Ok(Invocation {
        request_id,
        context,
        event,
    })
}

fn post_invocation_response(
    client: &Client,
    endpoint: &RuntimeEndpoint,
    request_id: &str,
    response: &JsonValue,
) -> Result<()> {
    let response = client
        .post(endpoint.response_url(request_id))
        .json(response)
        .send()
        .context("failed to post Lambda invocation response")?;
    if !response.status().is_success() {
        bail!(
            "Lambda Runtime API response post failed: {}",
            response.status()
        );
    }
    Ok(())
}

fn post_invocation_error(
    client: &Client,
    endpoint: &RuntimeEndpoint,
    request_id: &str,
    error: &anyhow::Error,
) -> Result<()> {
    let payload = json!({
        "errorMessage": error.to_string(),
        "errorType": "MatchBoxLambdaError"
    });
    let response = client
        .post(endpoint.error_url(request_id))
        .json(&payload)
        .send()
        .context("failed to post Lambda invocation error")?;
    if !response.status().is_success() {
        bail!(
            "Lambda Runtime API error post failed: {}",
            response.status()
        );
    }
    Ok(())
}

fn context_from_headers(headers: &HeaderMap, request_id: &str) -> LambdaContextSeed {
    let mut seed = LambdaContextSeed {
        aws_request_id: request_id.to_string(),
        invoked_function_arn: header_string(headers, "lambda-runtime-invoked-function-arn")
            .unwrap_or_default(),
        remaining_time_in_millis: remaining_time_from_deadline(
            header_string(headers, "lambda-runtime-deadline-ms")
                .and_then(|value| value.parse::<i64>().ok()),
        ),
        ..LambdaContextSeed::default()
    };

    if let Ok(value) = std::env::var("AWS_LAMBDA_FUNCTION_NAME") {
        seed.function_name = value;
    }
    if let Ok(value) = std::env::var("AWS_LAMBDA_FUNCTION_VERSION") {
        seed.function_version = value;
    }
    if let Ok(value) = std::env::var("AWS_LAMBDA_FUNCTION_MEMORY_SIZE")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or(())
    {
        seed.memory_limit_in_mb = value;
    }
    if let Ok(value) = std::env::var("AWS_LAMBDA_LOG_GROUP_NAME") {
        seed.log_group_name = value;
    }
    if let Ok(value) = std::env::var("AWS_LAMBDA_LOG_STREAM_NAME") {
        seed.log_stream_name = value;
    }

    seed
}

fn remaining_time_from_deadline(deadline_ms: Option<i64>) -> i32 {
    let Some(deadline_ms) = deadline_ms else {
        return LambdaContextSeed::default().remaining_time_in_millis;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    (deadline_ms - now_ms).max(0).min(i32::MAX as i64) as i32
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_accepts_plain_host_or_full_url() {
        let plain = RuntimeEndpoint::new("127.0.0.1:9001");
        assert_eq!(
            plain.next_url(),
            "http://127.0.0.1:9001/2018-06-01/runtime/invocation/next"
        );

        let full = RuntimeEndpoint::new("http://127.0.0.1:9001/");
        assert_eq!(
            full.response_url("abc"),
            "http://127.0.0.1:9001/2018-06-01/runtime/invocation/abc/response"
        );
    }

    #[test]
    fn remaining_time_uses_deadline_header() {
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 5_000;
        let remaining = remaining_time_from_deadline(Some(future));
        assert!(remaining > 0);
        assert!(remaining <= 5_000);
    }

    #[test]
    fn context_reads_lambda_headers_and_environment_defaults() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "lambda-runtime-invoked-function-arn",
            "arn:test".parse().unwrap(),
        );
        let seed = context_from_headers(&headers, "req");

        assert_eq!(seed.aws_request_id, "req");
        assert_eq!(seed.invoked_function_arn, "arn:test");
    }
}
