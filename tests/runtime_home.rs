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
fn explicit_missing_config_is_an_error() {
    let (_dir, user_home, cwd) = layout();
    let (ok, _, err) = run(&cwd, &user_home, &["--bx-config", "nope.json", "app.bxs"]);
    assert!(!ok);
    assert!(err.contains("config file not found"), "{err}");
}
