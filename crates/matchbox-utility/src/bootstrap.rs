//! Process startup: load `.env` files, resolve the home, seed it, load the config layers, start logging.

use crate::{config, env_file, home, logger};
use std::{
    io,
    path::{Path, PathBuf},
};

pub struct Options<'a> {
    /// `--bx-home`
    pub home: Option<&'a str>,
    /// `--bx-config`
    pub config: Option<&'a str>,
    pub cwd: PathBuf,
    /// The OS user's home directory, where `~/.matchbox.env` and the default `~/.matchbox` live.
    pub user_home: Option<PathBuf>,
}

pub struct Runtime {
    pub home: PathBuf,
    /// The config files that were layered, lowest priority first.
    pub config_files: Vec<PathBuf>,
    pub config: config::Config,
}

/// Bootstrap from the real process environment and working directory.
///
/// Call once at startup, before other threads exist.
pub fn bootstrap(home_flag: Option<&str>, config_flag: Option<&str>) -> io::Result<Runtime> {
    bootstrap_with(Options {
        home: home_flag,
        config: config_flag,
        cwd: std::env::current_dir()?,
        user_home: home::user_home_dir(),
    })
}

pub fn bootstrap_with(options: Options) -> io::Result<Runtime> {
    // 1. ~/.matchbox.env, then ./.env. Applied first so they can set MATCHBOX_HOME and MATCHBOX_CONFIG.
    let user_env = options
        .user_home
        .as_deref()
        .map(|h| h.join(".matchbox.env"));
    env_file::apply(&env_file::load(
        user_env.as_deref(),
        &options.cwd.join(".env"),
    )?);

    // 2. Home
    let env_home = std::env::var("MATCHBOX_HOME").ok();
    let home = home::resolve(
        options.home,
        env_home.as_deref(),
        options.user_home.as_deref(),
    )?;
    let home = absolute(&options.cwd, &home);
    home::ensure(&home)?;

    // 3. Config layers: home, ./boxlang.json, then --bx-config or MATCHBOX_CONFIG
    let env_config = std::env::var("MATCHBOX_CONFIG").ok();
    let config_files =
        config::resolve_layers(options.config, env_config.as_deref(), &options.cwd, &home)?;
    let config = config::load(&config_files, &home, &options.cwd)?;

    // 4. Logging
    logger::init(config.logging.clone())?;
    logger::debug(format!(
        "MatchBox home: {}, config: {}",
        home.display(),
        config_files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    Ok(Runtime {
        home,
        config_files,
        config,
    })
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstraps_env_home_config_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let user_home = dir.path().join("user");
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&user_home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            user_home.join(".matchbox.env"),
            "MB_BOOT_USER=user\nMB_BOOT_BOTH=user\n",
        )
        .unwrap();
        std::fs::write(cwd.join(".env"), "MB_BOOT_BOTH=project\n").unwrap();

        let runtime = bootstrap_with(Options {
            home: None,
            config: None,
            cwd: cwd.clone(),
            user_home: Some(user_home.clone()),
        })
        .unwrap();

        assert_eq!(std::env::var("MB_BOOT_USER").unwrap(), "user");
        assert_eq!(std::env::var("MB_BOOT_BOTH").unwrap(), "project");
        assert_eq!(runtime.home, user_home.join(".matchbox"));
        assert!(runtime.home.join("config/boxlang.json").is_file());
        assert!(runtime.home.join("logs").is_dir());
        assert_eq!(
            runtime.config_files,
            vec![runtime.home.join("config/boxlang.json")]
        );
    }
}
