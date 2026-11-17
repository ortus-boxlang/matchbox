//! `.env` loading, the MatchBox home (`matchbox.json`, `logs/`) and config precedence for the CLI.

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
    assert!(home.join("config/matchbox.json").is_file());
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
    assert!(flag_home.join("config/matchbox.json").is_file());
    assert!(!user_home.join(".matchbox").exists());

    let env_home = dir.path().join("env-home");
    fs::write(
        cwd.join(".env"),
        format!("MATCHBOX_HOME={}\n", env_home.display()),
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["app.bxs"]);
    assert!(ok, "{err}");
    assert!(env_home.join("config/matchbox.json").is_file());
}

#[test]
fn project_config_controls_the_runtime_log() {
    let (_dir, user_home, cwd) = layout();
    fs::write(
        cwd.join(".matchbox.json"),
        r#"{ "logging": { "rootLevel": "INFO", "defaultEncoder": "json" } }"#,
    )
    .unwrap();
    let (ok, _, err) = run(&cwd, &user_home, &["app.bxs"]);
    assert!(ok, "{err}");
    let log = fs::read_to_string(user_home.join(".matchbox/logs/matchbox.log")).unwrap();
    assert!(
        log.contains(r#""level":"INFO""#) && log.contains("MatchBox home"),
        "{log}"
    );
}

#[test]
fn explicit_missing_config_is_an_error() {
    let (_dir, user_home, cwd) = layout();
    let (ok, _, err) = run(&cwd, &user_home, &["--bx-config", "nope.json", "app.bxs"]);
    assert!(!ok);
    assert!(err.contains("config file not found"), "{err}");
}
