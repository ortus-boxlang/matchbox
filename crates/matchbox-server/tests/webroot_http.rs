use matchbox_server::webroot_core::{
    EmbeddedAssetStore, WebrootConfig, WebrootContext, WebrootEngine, WebrootRequest,
};
use std::collections::HashMap;

fn engine(source: &str) -> WebrootEngine<EmbeddedAssetStore> {
    WebrootEngine::new(
        EmbeddedAssetStore::new(HashMap::from([(
            "index.bxm".into(),
            source.as_bytes().to_vec(),
        )])),
        WebrootConfig::default(),
    )
}

#[test]
fn templates_control_status_headers_and_content_buffer() {
    let engine = engine(
        r#"<bx:header statusCode="202"><bx:header name="X-Test" value="first"><bx:header name="X-Test" value="second">discard<bx:content type="text/plain">keep"#,
    );
    let response = engine
        .handle(WebrootRequest {
            method: "GET".into(),
            path: "/".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.status, 202);
    assert_eq!(response.body, b"keep");
    let values: Vec<_> = response
        .headers
        .iter()
        .filter(|(name, _)| name.as_str() == "x-test")
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(values, ["first", "second"]);
}

#[test]
fn content_preserves_output_when_reset_is_false_and_executes_its_body() {
    let engine =
        engine(r#"before<bx:content type="text/plain" reset="false">inside</bx:content>after"#);
    let response = engine
        .handle(WebrootRequest {
            method: "GET".into(),
            path: "/".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.body, b"beforeinsideafter");
}

#[test]
fn request_data_is_available_for_patch_and_preserves_raw_body() {
    let engine = engine(
        r#"<bx:script>d = getHTTPRequestData(); writeOutput(d.method & "|" & d.protocol & "|" & d.content);</bx:script>"#,
    );
    let response = engine
        .handle(WebrootRequest {
            method: "PATCH".into(),
            path: "/".into(),
            headers: HashMap::from([("content-type".into(), "application/json".into())]),
            body: br#"{"a":1}"#.to_vec(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, br#"PATCH|HTTP/1.1|{"a":1}"#);
}

#[test]
fn binary_bodies_remain_binary_and_named_include_body_can_omit_them() {
    for (source, expected) in [
        (
            r#"<bx:script>d = getHTTPRequestData(); writeOutput(isBinary(d.content) & "|" & toBase64(d.content));</bx:script>"#,
            "true|AP8B",
        ),
        (
            r#"<bx:script>d = getHTTPRequestData(includeBody=false); writeOutput(structKeyExists(d, "content"));</bx:script>"#,
            "false",
        ),
    ] {
        let response = engine(source)
            .handle(WebrootRequest {
                method: "POST".into(),
                path: "/".into(),
                headers: HashMap::from([(
                    "content-type".into(),
                    "application/octet-stream".into(),
                )]),
                body: vec![0, 255, 1],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(response.body, expected.as_bytes());
    }
}

#[test]
fn absent_body_is_text_but_announced_empty_chunked_binary_body_is_binary() {
    let engine =
        engine(r#"<bx:script>writeOutput(isBinary(getHTTPRequestData().content));</bx:script>"#);
    for (chunked, expected) in [(false, "false"), (true, "true")] {
        let mut headers =
            HashMap::from([("content-type".into(), "application/octet-stream".into())]);
        if chunked {
            headers.insert("transfer-encoding".into(), "chunked".into());
        } else {
            headers.insert("content-length".into(), "0".into());
        }
        let response = engine
            .handle(WebrootRequest {
                method: "POST".into(),
                path: "/".into(),
                headers,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(response.body, expected.as_bytes());
    }
}

#[test]
fn transport_context_preserves_original_url_and_alias_script_name() {
    let engine = engine(
        r##"<bx:output>#cgi.script_name#|#cgi.query_string#|#cgi.request_url#|#cgi.remote_addr#|#cgi.server_protocol#|#cgi.server_name#|#cgi.server_port#|#cgi.http_x_test#</bx:output>"##,
    );
    let context = WebrootContext {
        uri: "/api/route".into(),
        script_name: "/api/index.bxm".into(),
        query_string: "x=%2B&x=last".into(),
        protocol: "HTTP/2.0".into(),
        scheme: "https".into(),
        remote_addr: "192.0.2.1".into(),
        server_port: 8443,
        ..Default::default()
    };
    let response = engine
        .handle_with_context(
            WebrootRequest {
                method: "GET".into(),
                path: "/".into(),
                headers: HashMap::from([
                    ("host".into(), "[::1]:9443".into()),
                    ("x-test".into(), "yes".into()),
                ]),
                ..Default::default()
            },
            context,
        )
        .unwrap();
    assert_eq!(
        response.body,
        b"/api/index.bxm|x=%2B&x=last|https://[::1]:9443/api/route|192.0.2.1|HTTP/2.0|::1|9443|yes"
    );
}

#[test]
fn head_keeps_representation_length_and_204_has_no_body_or_length() {
    let response = engine("hello")
        .handle(WebrootRequest {
            method: "HEAD".into(),
            path: "/".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(response.body.is_empty());
    assert!(
        response
            .headers
            .iter()
            .any(|(name, value)| name == "content-length" && value == "5")
    );
    let response = engine(r#"<bx:header statusCode="204">ignored"#)
        .handle(WebrootRequest {
            method: "GET".into(),
            path: "/".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.status, 204);
    assert!(response.body.is_empty());
    assert!(
        !response
            .headers
            .iter()
            .any(|(name, _)| name == "content-length")
    );
}

#[test]
fn web_headers_reject_injection_invalid_status_and_transport_framing() {
    for source in [
        r##"<bx:header name="X-Test" value="#chr(13)#injected">"##,
        r#"<bx:header statusCode="999">"#,
        r#"<bx:header name="Content-Length" value="999">"#,
    ] {
        assert!(
            engine(source)
                .handle(WebrootRequest {
                    method: "GET".into(),
                    path: "/".into(),
                    ..Default::default()
                })
                .unwrap()
                .status
                == 500,
            "{source}"
        );
    }
}
