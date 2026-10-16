use matchbox_server::{
    multipart,
    webroot_core::{EmbeddedAssetStore, WebrootConfig, WebrootEngine, WebrootRequest},
};
use std::collections::HashMap;
#[tokio::test]
async fn parsed_uploads_are_binary_safe_and_cleaned_after_template_errors_not_gc() {
    let body=b"--test\r\nContent-Disposition: form-data; name=\"payload\"; filename=\"../../secret.bxs\"\r\nContent-Type: application/octet-stream\r\n\r\n\0\xffbinary\r\n--test--\r\n";
    let mut request = WebrootRequest {
        method: "POST".into(),
        path: "/index.bxs".into(),
        headers: HashMap::from([(
            "content-type".into(),
            "multipart/form-data; boundary=test".into(),
        )]),
        body: body.to_vec(),
        ..Default::default()
    };
    multipart::parse(&mut request).await.unwrap();
    let file = request.uploads[0].clone();
    assert_eq!(file.filename, "../../secret.bxs");
    assert_eq!(file.content_type, "application/octet-stream");
    assert_eq!(std::fs::read(file.path()).unwrap(), b"\0\xffbinary");
    let engine = WebrootEngine::new(
        EmbeddedAssetStore::new(HashMap::from([(
            "index.bxs".into(),
            b"throw 'request failed';".to_vec(),
        )])),
        WebrootConfig::default(),
    );
    assert_eq!(engine.handle(request).unwrap().status, 500);
    assert!(!file.path().exists());
}
#[tokio::test]
async fn malformed_multipart_is_atomic_and_empty_file_fields_do_not_create_uploads() {
    let mut request=WebrootRequest {method:"POST".into(),headers:HashMap::from([("content-type".into(),"multipart/form-data; boundary=test".into())]),body:b"--test\r\nContent-Disposition: form-data; name=\"empty\"; filename=\"\"\r\n\r\n\r\n--test--\r\n".to_vec(),..Default::default()};
    multipart::parse(&mut request).await.unwrap();
    assert!(request.uploads.is_empty());
    assert_eq!(request.form.get("empty").unwrap(), "");
    request.body =
        b"--test\r\nContent-Disposition: form-data; name=\"new\"\r\n\r\nnot closed".to_vec();
    assert!(multipart::parse(&mut request).await.is_err());
    assert_eq!(request.form.len(), 1);
    assert!(!request.form.contains_key("new"));
}
#[tokio::test]
async fn multipart_parameter_limit_is_enforced_before_installing_any_fields() {
    let mut body = Vec::new();
    for _ in 0..1001 {
        body.extend_from_slice(
            b"--test\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\na\r\n",
        );
    }
    body.extend_from_slice(b"--test--\r\n");
    let mut request = WebrootRequest {
        headers: HashMap::from([(
            "content-type".into(),
            "multipart/form-data; boundary=test".into(),
        )]),
        body,
        ..Default::default()
    };
    assert!(
        multipart::parse(&mut request)
            .await
            .unwrap_err()
            .to_string()
            .contains("Too many")
    );
    assert!(request.form.is_empty());
    assert!(request.uploads.is_empty());
}
