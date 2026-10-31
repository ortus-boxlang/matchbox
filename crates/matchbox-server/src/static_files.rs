//! Native HTTP file transport. The host must confine/authorize the selected path first.
use crate::webroot_core::WebrootRequest;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::path::PathBuf;
use tower_http::services::ServeFile;

/// Stream a selected static file with standard Range/date preconditions and HEAD lengths.
/// POST retains MiniServer's static-resource semantics; unsupported methods never open the file.
pub async fn static_file_response(
    path: PathBuf,
    request: WebrootRequest,
    content_type: Option<&str>,
) -> anyhow::Result<Response> {
    let WebrootRequest {
        method, headers, ..
    } = request;
    if !matches!(method.as_str(), "GET" | "HEAD" | "POST") {
        return Ok((
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD, POST")],
        )
            .into_response());
    }
    let mut request = Request::builder().method(if method == "POST" {
        "GET"
    } else {
        method.as_str()
    });
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let mut request = request.body(Body::empty())?;
    // tower-http 0.5 handles Range and date preconditions but does not implement If-Range.
    if request.headers().contains_key(header::RANGE)
        && let Some(value) = request.headers().get(header::IF_RANGE)
    {
        let valid = match value
            .to_str()
            .ok()
            .and_then(|value| httpdate::parse_http_date(value).ok())
        {
            Some(date) => tokio::fs::metadata(&path)
                .await
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .is_some_and(|modified| {
                    date.checked_add(std::time::Duration::from_secs(1))
                        .is_some_and(|limit| modified < limit)
                }),
            None => false,
        };
        if !valid {
            request.headers_mut().remove(header::RANGE);
        }
    }
    let mut service = match content_type {
        Some(mime) => ServeFile::new_with_mime(path, &mime.parse()?),
        None => ServeFile::new(path),
    };
    let response = service.try_call(request).await?;
    Ok(response.map(Body::new))
}
