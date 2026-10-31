//! Opt-in Linux/glibc public-runtime retention regression (not a timing benchmark).
#![cfg(all(target_os = "linux", target_env = "gnu"))]
use matchbox_server::webroot_core::{EmbeddedAssetStore, WebrootConfig, WebrootContext, WebrootEngine, WebrootRequest};
use std::collections::HashMap;

#[repr(C)]
#[derive(Default)]
struct Mallinfo {
    arena: usize, ordblks: usize, smblks: usize, hblks: usize, hblkhd: usize,
    usmblks: usize, fsmblks: usize, uordblks: usize, fordblks: usize, keepcost: usize,
}
unsafe extern "C" { fn mallinfo2() -> Mallinfo; }
fn outstanding() -> usize {
    let m = unsafe { mallinfo2() };
    m.uordblks + m.hblkhd
}

#[test]
#[ignore = "manual Linux/glibc retention regression; run alone with --ignored --test-threads=1"]
fn repeated_cgi_field_set_has_bounded_retention() {
    let source = br#"writeOutput(cgi.REQUEST_METHOD & '|' & cgi.script_name & '|' & cgi.query_string & '|' & cgi.request_url & '|' & cgi.remote_addr & '|' & cgi.server_protocol & '|' & cgi.server_name & '|' & cgi.server_port & '|' & cgi.http_x_test & '|' & structCount(cgi));"#;
    let engine = WebrootEngine::new(EmbeddedAssetStore::new(HashMap::from([("index.bxs".into(), source.to_vec())])), WebrootConfig::default());
    let mut warm = 0;
    for i in 1..=6000 {
        // Fresh randomized input maps, same field set; vary values to detect stale request data.
        let value = (i % 7).to_string();
        let response = engine.handle_with_context(WebrootRequest {
            method: "GET".into(), path: "/index.bxs".into(),
            headers: HashMap::from([("host".into(), "example.test:9443".into()), ("x-test".into(), value.clone())]),
            ..Default::default()
        }, WebrootContext {
            uri: "/api/route".into(), script_name: "/api/index.bxs".into(),
            query_string: format!("n={value}"), protocol: "HTTP/2.0".into(),
            scheme: "https".into(), remote_addr: "192.0.2.1".into(), server_port: 8443,
            ..Default::default()
        }).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, format!("GET|/api/index.bxs|n={value}|https://example.test:9443/api/route|192.0.2.1|HTTP/2.0|example.test|9443|{value}|16").as_bytes());
        drop(response);
        if i % 1000 == 0 {
            let bytes = outstanding();
            println!("RETENTION {{\"requests\":{i},\"outstanding\":{bytes}}}");
            if i == 1000 { warm = bytes; }
        }
    }
    let growth = outstanding().saturating_sub(warm);
    // 128 bytes/request leaves >40x headroom below the reproduced ~6 KiB/request defect.
    assert!(growth <= 5000 * 128, "CGI_RETENTION_LIMIT: {growth} > 640000 bytes");
}
