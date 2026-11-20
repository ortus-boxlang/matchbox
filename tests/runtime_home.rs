//! `.env` loading, the MatchBox home (`boxlang.json`, `logs/`), config layering and logging for the CLI.

use std::{fs, path::Path, process::Command};

fn run(cwd: &Path, user_home: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", user_home)
        .env("USERPROFILE", user_home)
        .env_remove("MATCHBOX_HOME")
        .env_remove("MATCHBOX_CONFIG")
        .env("MB_RT_OS", "os")
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn layout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let user_home = dir.path().join("user");
    let cwd = dir.path().join("project");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("app.bxs"),
        "println( getSystemSetting( 'MB_RT_USER' ) & '|' & getSystemSetting( 'MB_RT_BOTH' ) & '|' & getSystemSetting( 'MB_RT_OS' ) );\n",
    )
    .unwrap();
    (dir, user_home, cwd)
}

#[test]
fn loads_user_and_project_env_and_creates_the_home() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        user_home.join(".matchbox.env"),
        "MB_RT_USER=user\nMB_RT_BOTH=user\n",
    )
    .unwrap();
    fs::write(cwd.join(".env"), "MB_RT_BOTH=project\nMB_RT_OS=file\n").unwrap();

    let (ok, out, err) = run(&cwd, &user_home, &["app.bxs"]);
    assert!(ok, "{err}");
    assert_eq!(
        out.trim(),
        "user|project|file",
        "project wins over user, files win over the OS env"
    );

    let home = user_home.join(".matchbox");
    assert!(home.join("config/boxlang.json").is_file());
    for dir in ["bin", "classes", "global", "logs", "lib", "modules"] {
        assert!(home.join(dir).is_dir(), "{dir} missing");
    }
}

#[test]
fn matchbox_home_env_and_flag_choose_the_home() {
    let (dir, user_home, cwd) = layout();
    let flag_home = dir.path().join("flag-home");
    let (ok, _, err) = run(
        &cwd,
        &user_home,
        &["--bx-home", flag_home.to_str().unwrap(), "app.bxs"],
    );
    assert!(ok, "{err}");
    assert!(flag_home.join("config/boxlang.json").is_file());
    assert!(!user_home.join(".matchbox").exists());

    let env_home = dir.path().join("env-home");
    fs::write(
        cwd.join(".env"),
        format!("MATCHBOX_HOME={}\n", env_home.display()),
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["app.bxs"]);
    assert!(ok, "{err}");
    assert!(env_home.join("config/boxlang.json").is_file());
}

#[test]
fn runtime_logger_uses_the_configured_named_sink_without_a_module() {
    let (_dir, user_home, cwd) = layout();
    fs::write(cwd.join("boxlang.json"), r#"{"logging":{"loggers":{"mcp":{"level":"INFO","encoder":"json"}}}}"#).unwrap();
    fs::write(cwd.join("app.bxs"), r#"
        rt = getBoxRuntime();
        assert rt === getBoxRuntime();
        logger = rt.getLoggingService().getLogger("MCP");
        assert logger === getBoxRuntime().getLoggingService().getLogger("mcp.log");
        assert logger.isInfoEnabled() && !logger.isDebugEnabled();
        logger.debug("below threshold");
        logger.info("named logger message");
    "#).unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["app.bxs"]);
    assert!(ok, "{err}");
    let log = fs::read_to_string(user_home.join(".matchbox/logs/mcp.log")).unwrap();
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 1, "{log}");
    let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(entry["message"], "named logger message");
    assert_eq!(entry["level"], "INFO");
}

#[test]
fn write_log_and_the_log_component_follow_the_layered_config() {
    let (_dir, user_home, cwd) = layout();
    // ./boxlang.json overlays the home's config: application logs at DEBUG, orders is its own file.
    fs::write(
        cwd.join("boxlang.json"),
        r#"{ "logging": { "loggers": { "application": { "level": "DEBUG" }, "orders": { "level": "INFO", "encoder": "json" } } } }"#,
    )
    .unwrap();
    fs::write(
        cwd.join("log.bxs"),
        "writeLog( 'Hello Logger!' );\n\
         writeLog( text = 'Hola debug', type = 'Debug' );\n\
         writeLog( text = 'Order shipped', log = 'orders' );\n\
         writeLog( text = 'Dropped', type = 'trace' );\n\
         writeLog( text = 'No app', type = 'warning', application = false );\n",
    )
    .unwrap();
    fs::write(cwd.join("log.bxm"), "<bx:log text=\"tag default\" /><bx:log text=\"tag orders\" type=\"error\" log=\"orders\" />").unwrap();

    let (ok, _, err) = run(&cwd, &user_home, &["log.bxs"]);
    assert!(ok, "{err}");
    let (ok, _, err) = run(&cwd, &user_home, &["log.bxm"]);
    assert!(ok, "{err}");

    let logs = user_home.join(".matchbox/logs");
    let application = fs::read_to_string(logs.join("application.log")).unwrap();
    assert!(
        application.contains("[INFO] [no-application] Hello Logger!"),
        "{application}"
    );
    assert!(
        application.contains("[DEBUG] [no-application] Hola debug"),
        "{application}"
    );
    assert!(
        application.contains("[WARN] [no-application] No app"),
        "{application}"
    );
    assert!(
        application.contains("[INFO] [no-application] tag default"),
        "{application}"
    );
    assert!(
        !application.contains("Dropped"),
        "trace is below the DEBUG level: {application}"
    );

    let orders = fs::read_to_string(logs.join("orders.log")).unwrap();
    assert!(
        orders.contains(r#""level":"INFO""#) && orders.contains("Order shipped"),
        "{orders}"
    );
    assert!(
        orders.contains(r#""level":"ERROR""#) && orders.contains("tag orders"),
        "{orders}"
    );
}

#[test]
fn default_levels_drop_information_from_the_application_log() {
    // The seeded config logs `application` at WARN, as BoxLang does.
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join("log.bxs"),
        "writeLog( 'quiet' );\nwriteLog( text = 'loud', type = 'error' );\n",
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["log.bxs"]);
    assert!(ok, "{err}");
    let application = fs::read_to_string(user_home.join(".matchbox/logs/application.log")).unwrap();
    assert!(
        !application.contains("quiet") && application.contains("loud"),
        "{application}"
    );
}

#[test]
fn application_name_prefixes_log_messages() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join("boxlang.json"),
        r#"{ "logging": { "loggers": { "application": { "level": "INFO" } } } }"#,
    )
    .unwrap();
    fs::write(
        cwd.join("Application.bx"),
        "class { this.name = 'shop'; }\n",
    )
    .unwrap();
    fs::write(
        cwd.join("log.bxs"),
        "writeLog( 'ordered' );\nwriteLog( text = 'bare', application = false );\n",
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["log.bxs"]);
    assert!(ok, "{err}");
    let application = fs::read_to_string(user_home.join(".matchbox/logs/application.log")).unwrap();
    assert!(
        application.contains("[INFO] [shop] ordered"),
        "{application}"
    );
    assert!(
        application.contains("[INFO] [no-application] bare"),
        "{application}"
    );
}

#[test]
fn invalid_log_type_and_names_outside_the_logs_directory_fail() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join("bad.bxs"),
        "writeLog( text = 'x', type = 'loud' );\n",
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["bad.bxs"]);
    assert!(!ok && err.contains("Invalid log level: loud"), "{err}");
    fs::write(
        cwd.join("escape.bxs"),
        "writeLog( text = 'x', log = '../escape' );\n",
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["escape.bxs"]);
    assert!(!ok && err.contains("Invalid logger name"), "{err}");
}

#[test]
fn rapid_log_rotation_preserves_every_message() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join("boxlang.json"),
        r#"{ "logging": { "maxFileSize": "150B", "maxLogDays": 0, "totalCapSize": "0" } }"#,
    )
    .unwrap();
    // Many rotations in one burst reproduce timestamp collisions without sleeps.
    const MESSAGES: usize = 2000;
    fs::write(
        cwd.join("rotate.bxs"),
        format!(
            "for (i = 1; i <= {MESSAGES}; i++) {{ writeLog(text = 'record-' & i, type = 'error'); }}"
        ),
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["rotate.bxs"]);
    assert!(ok, "{err}");

    // Read both the active log and all archives, independent of archive naming/layout.
    fn records_in(dir: &Path, records: &mut Vec<usize>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                records_in(&path, records);
            } else {
                for line in fs::read_to_string(&path).unwrap().lines() {
                    if let Some((_, id)) = line.split_once("[no-application] record-") {
                        records.push(id.parse().expect("complete log record"));
                    }
                }
            }
        }
    }
    let mut records = Vec::new();
    records_in(&user_home.join(".matchbox/logs"), &mut records);
    assert_eq!(
        records.len(), MESSAGES,
        "rotation must not lose or duplicate records when retention is disabled"
    );
    records.sort_unstable();
    assert_eq!(records, (1..=MESSAGES).collect::<Vec<_>>());
}

#[test]
fn logger_names_with_and_without_log_extension_share_settings() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join("boxlang.json"),
        r#"{ "logging": { "loggers": { "orders": { "level": "ERROR", "encoder": "json" } } } }"#,
    )
    .unwrap();
    fs::write(
        cwd.join("aliases.bxs"),
        "writeLog(text = 'hidden-bare', type = 'warning', log = 'orders');\n\
         writeLog(text = 'hidden-extension', type = 'warning', log = 'orders.log');\n\
         writeLog(text = 'kept-bare', type = 'error', log = 'orders');\n\
         writeLog(text = 'kept-extension', type = 'error', log = 'orders.log');\n",
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["aliases.bxs"]);
    assert!(ok, "{err}");
    let orders = fs::read_to_string(user_home.join(".matchbox/logs/orders.log")).unwrap();
    assert!(
        !orders.contains("hidden-"),
        "both names must honor the configured ERROR threshold: {orders}"
    );
    let entries: Vec<serde_json::Value> = orders
        .lines()
        .map(|line| serde_json::from_str(line).expect("both names must use the configured JSON encoder"))
        .collect();
    assert_eq!(entries.len(), 2, "{orders}");
    assert_eq!(entries[0]["message"], "[no-application] kept-bare");
    assert_eq!(entries[1]["message"], "[no-application] kept-extension");
    assert!(entries.iter().all(|entry| entry["level"] == "ERROR"));
}

#[cfg(feature = "server")]
mod http_logging {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpStream,
        process::{Child, Stdio},
        time::{Duration, Instant},
    };

    // Like the server crate's HTTP transport tests, exercise a real child and TCP request.
    // Use the main CLI here so runtime bootstrap and logging configuration run too.
    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn assert_http_logging(filename: &str, source: &str, message: &str) {
        let (_dir, user_home, cwd) = layout();
        fs::write(cwd.join(filename), source).unwrap();
        let stdout_path = cwd.join("server.stdout");
        let stderr_path = cwd.join("server.stderr");
        let mut server = Server(
            Command::new(env!("CARGO_BIN_EXE_matchbox"))
                .args(["--serve", "--host", "127.0.0.1", "--port", "0", "--webroot", "."])
                .current_dir(&cwd)
                .env("HOME", &user_home)
                .env("USERPROFILE", &user_home)
                .env_remove("MATCHBOX_HOME")
                .env_remove("MATCHBOX_CONFIG")
                .stdout(Stdio::from(fs::File::create(&stdout_path).unwrap()))
                .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()))
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let port: u16 = loop {
            let stdout = fs::read_to_string(&stdout_path).unwrap();
            if let Some(address) = stdout.lines().find_map(|line| {
                line.strip_prefix("MatchBox Server listening on http://127.0.0.1:")
            }) {
                break address.parse().expect("server's bound port");
            }
            let exited = server.0.try_wait().unwrap();
            assert!(
                exited.is_none() && Instant::now() < deadline,
                "server did not start: status={exited:?}\n{stdout}\n{}",
                fs::read_to_string(&stderr_path).unwrap()
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(stream, "GET /{filename} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
        assert!(
            headers.starts_with("HTTP/1.0 200"),
            "logging must not fail the request: {response}\n{}",
            fs::read_to_string(&stderr_path).unwrap()
        );
        assert_eq!(body.trim(), "logged");
        let log = fs::read_to_string(user_home.join(".matchbox/logs/application.log"))
            .expect("HTTP request must create the application log");
        assert!(log.contains(&format!("[ERROR] [no-application] {message}")), "{log}");
    }

    #[test]
    fn write_log_in_an_http_request_writes_the_message() {
        assert_http_logging(
            "log.bxs",
            "writeLog(text = 'http-function', type = 'error'); writeOutput('logged');",
            "http-function",
        );
    }

    #[test]
    fn log_component_in_an_http_request_writes_the_message() {
        assert_http_logging(
            "log.bxm",
            "<bx:log text=\"http-component\" type=\"error\" />logged",
            "http-component",
        );
    }
}

#[test]
fn explicit_missing_config_is_an_error() {
    let (_dir, user_home, cwd) = layout();
    let (ok, _, err) = run(&cwd, &user_home, &["--bx-config", "nope.json", "app.bxs"]);
    assert!(!ok);
    assert!(err.contains("config file not found"), "{err}");
}
