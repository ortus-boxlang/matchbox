use axum::body::to_bytes;
use futures_util::StreamExt;
use matchbox_server::{static_files::static_file_response, webroot_core::WebrootRequest};
use std::collections::HashMap;
fn request(method: &str, headers: &[(&str, &str)]) -> WebrootRequest {
    WebrootRequest {
        method: method.into(),
        headers: headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<HashMap<_, _>>(),
        ..Default::default()
    }
}
#[tokio::test]
async fn static_transport_streams_bounded_chunks_and_drops_file_handles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.bin");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(128 * 1024 * 1024).unwrap();
    drop(file);
    let response = static_file_response(path.clone(), request("GET", &[]), None)
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-length"], "134217728");
    let mut stream = response.into_body().into_data_stream();
    let chunk = stream.next().await.unwrap().unwrap();
    assert!(!chunk.is_empty());
    assert!(chunk.len() <= 65536);
    drop(stream);
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn ranges_head_and_preconditions_follow_the_selected_file_not_the_uri() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file.txt");
    std::fs::write(&path, b"0123456789").unwrap();
    let response = static_file_response(
        path.clone(),
        request("GET", &[("range", "bytes=2-5")]),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), "2345");
    let response = static_file_response(
        path.clone(),
        request(
            "GET",
            &[
                ("range", "bytes=1-2"),
                ("if-range", "Thu, 01 Jan 1970 00:00:00 GMT"),
            ],
        ),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap(),
        "0123456789"
    );
    let response = static_file_response(
        path.clone(),
        request("HEAD", &[]),
        Some("application/octet-stream"),
    )
    .await
    .unwrap();
    assert_eq!(response.headers()["content-length"], "10");
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert!(
        to_bytes(response.into_body(), 100)
            .await
            .unwrap()
            .is_empty()
    );
    let response = static_file_response(
        path.clone(),
        request("GET", &[("range", "bytes=99-")]),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 416);
    let response = static_file_response(
        path.clone(),
        request(
            "GET",
            &[("if-unmodified-since", "Thu, 01 Jan 1970 00:00:00 GMT")],
        ),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 412);
    let response = static_file_response(path.clone(), request("PUT", &[]), None)
        .await
        .unwrap();
    assert_eq!(response.status(), 405);
    assert_eq!(response.headers()["allow"], "GET, HEAD, POST");
    std::fs::remove_file(&path).unwrap();
    let response = static_file_response(
        path,
        request(
            "GET",
            &[
                ("range", "bytes=1-2"),
                ("if-range", "Fri, 01 Jan 2100 00:00:00 GMT"),
            ],
        ),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 404);
}
