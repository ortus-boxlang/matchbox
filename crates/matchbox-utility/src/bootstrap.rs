//! Process startup: load `.env` files, resolve the home, seed it, load the config layers, start logging.

use crate::{config, env_file, home, logger};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The inputs of [`bootstrap_with`]. [`bootstrap`] fills them in from the real process.
pub struct Options<'a> {
    /// `--bx-home`
    pub home: Option<&'a str>,
    /// `--bx-config`
    pub config: Option<&'a str>,
    /// The working directory, where `./.env` and `./boxlang.json` are read.
    pub cwd: PathBuf,
    /// The OS user's home directory, where `~/.matchbox.env` and the default `~/.matchbox` live.
    pub user_home: Option<PathBuf>,
}

/// What [`bootstrap`] set up.
pub struct Runtime {
    /// The MatchBox home, as an absolute path. It exists.
    pub home: PathBuf,
    /// The config files that were layered, lowest priority first.
    pub config_files: Vec<PathBuf>,
    /// The merged configuration. Its `logging` block is what the process-wide log service was started with.
    pub config: config::Config,
}

/// Bootstrap from the real process environment and working directory.
///
/// Call once at startup, before other threads exist.
///
/// # Errors
///
/// See [`bootstrap_with`].
pub fn bootstrap(home_flag: Option<&str>, config_flag: Option<&str>) -> io::Result<Runtime> {
    bootstrap_with(Options {
        home: home_flag,
        config: config_flag,
        cwd: std::env::current_dir()?,
        user_home: home::user_home_dir(),
    })
}

/// Set up the runtime from explicit [`Options`].
///
/// The steps run in this order, and the first failure stops the rest:
///
/// 1. `~/.matchbox.env` then `./.env` are loaded into the process environment. They run first so a project `.env` can
///    set `MATCHBOX_HOME` and `MATCHBOX_CONFIG`.
/// 2. The home is resolved ([`crate::home::resolve`]) and created ([`crate::home::ensure`]).
/// 3. The config layers are resolved and merged ([`crate::config::resolve_layers`], [`crate::config::load`]).
/// 4. The process-wide log service is started ([`crate::logger::init`]).
///
/// # Errors
///
/// Returns the I/O error of the failing step: an unreadable `.env`, no user home to put the MatchBox home in, a home
/// that cannot be created, a missing `--bx-config` file, invalid JSON in a config layer, or a log file that cannot be
/// opened.
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

/// `path` as an absolute path, resolved against `cwd` when it is relative.
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
