use matchbox_server::{
    multipart,
    webroot_core::{FileSystemAssetStore, WebrootConfig, WebrootEngine, WebrootRequest},
};
use std::collections::HashMap;
async fn request(filename: &str) -> WebrootRequest {
    let mut request=WebrootRequest {method:"POST".into(),path:"/index.bxs".into(),headers:HashMap::from([("content-type".into(),"multipart/form-data; boundary=test".into())]),body:format!("--test\r\nContent-Disposition: form-data; name=\"payload\"; filename=\"{filename}\"\r\nContent-Type: image/png\r\n\r\nhello\r\n--test--\r\n").into_bytes(),..Default::default()};
    multipart::parse(&mut request).await.unwrap();
    request
}
fn engine(root: &std::path::Path, source: &str) -> WebrootEngine<FileSystemAssetStore> {
    std::fs::write(root.join("index.bxs"), source).unwrap();
    WebrootEngine::new(
        FileSystemAssetStore::new(root.into()),
        WebrootConfig::default(),
    )
}
#[tokio::test]
async fn failed_overwrite_preserves_the_existing_file_and_path_traversal_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("saved")).unwrap();
    std::fs::write(root.path().join("saved/result.txt"), b"original").unwrap();
    let request = request("test.txt").await;
    std::fs::remove_file(request.uploads[0].path()).unwrap();
    let engine = engine(
        root.path(),
        "fileUpload(destination='./saved/result.txt',nameConflict='overwrite');",
    );
    assert_eq!(engine.handle(request).unwrap().status, 500);
    assert_eq!(
        std::fs::read(root.path().join("saved/result.txt")).unwrap(),
        b"original"
    );
    let response = engine
        .handle(self::request("../../outside.txt").await)
        .unwrap();
    assert_eq!(response.status, 500);
    assert!(!root.path().join("outside.txt").exists());
}
#[tokio::test]
async fn upload_security_uses_original_extensions_not_claimed_mime_and_keeps_rejected_metadata() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("saved")).unwrap();
    let engine = engine(
        root.path(),
        "result=fileUpload(destination='./saved',accept='image/png',strict=false);writeOutput(result.fileWasSaved & '|' & result.clientFile & '|' & result.fileSize);",
    );
    let request = request("test.txt").await;
    let path = request.uploads[0].path().to_path_buf();
    assert_eq!(engine.handle(request).unwrap().body, b"false|test.txt|5");
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_dir(root.path().join("saved"))
            .unwrap()
            .count(),
        0
    );
}
#[tokio::test]
async fn multi_upload_results_and_application_security_are_rooted_across_namespaces() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("saved")).unwrap();
    std::fs::write(
        root.path().join("Application.bx"),
        "class {this.disallowedFileOperationExtensions=['txt'];}",
    )
    .unwrap();
    let engine = engine(
        root.path(),
        "try {fileUpload(destination='./saved');writeOutput('unsafe');}catch(any e){r=fileUploadAll(destination='./saved',nameConflict='makeunique',allowedExtensions=['txt'],strict=false);writeOutput(arrayLen(r) & '|' & r[1].fileWasSaved);}",
    );
    // Strict rejection deletes the source, so a second upload cannot reuse that rejected part.
    assert_eq!(
        engine.handle(request("test.txt").await).unwrap().status,
        500
    );
    std::fs::write(root.path().join("index.bxs"),"r=fileUploadAll(destination='./saved',nameConflict='makeunique',allowedExtensions=['txt'],strict=false);writeOutput(arrayLen(r) & '|' & r[1].fileWasSaved);").unwrap();
    let engine = WebrootEngine::new(
        FileSystemAssetStore::new(root.path().into()),
        WebrootConfig::default(),
    );
    for _ in 0..12 {
        assert_eq!(
            engine.handle(request("test.txt").await).unwrap().body,
            b"1|true"
        );
    }
}
