//! Exercise the shipped HTTP binary, not just the engine adapter.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn http_transport_preserves_body_protocol_peer_and_repeated_headers() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("request.bxs"), r#"
        d = getHTTPRequestData();
        bx:header statusCode=202;
        bx:header name="X-Repeat" value="first";
        bx:header name="X-Repeat" value="second";
        writeOutput("discard");
        bx:content type="text/plain";
        writeOutput(d.method & "|" & d.protocol & "|" & d.content & "|" & cgi.query_string & "|" & cgi.remote_addr & "|" & d.headers["x-test"] & "|" & cgi.http_x_test);
    "#).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_matchbox_server"))
            .args(["--port", "0", "--host", "127.0.0.1", "--webroot"])
            .arg(root.path())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stdout = BufReader::new(server.0.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(line.contains("listening on http://127.0.0.1:"), "{line}");
    let port = line
        .trim()
        .rsplit(':')
        .next()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let body = r#"{"name":"BoxLang"}"#;
    let response = request(
        port,
        &format!(
            "PATCH /request.bxs?raw=%2B HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nX-Test: first\r\nX-Test: second\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.0 202"), "{response}");
    assert!(
        headers.contains("x-repeat: first\r\nx-repeat: second"),
        "{headers}"
    );
    assert!(headers.contains("content-type: text/plain"), "{headers}");
    assert_eq!(
        body,
        r#"PATCH|HTTP/1.0|{"name":"BoxLang"}|raw=%2B|127.0.0.1|first|first"#
    );
    let response = request(
        port,
        "HEAD /request.bxs HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.0 202"), "{response}");
    assert!(headers.contains("content-length:"), "{headers}");
    assert!(body.is_empty());
}
