//! The MatchBox home directory: `--bx-home`, then `MATCHBOX_HOME`, then `~/.matchbox`.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Default `config/boxlang.json` seeded into a new home.
pub const DEFAULT_CONFIG: &str = include_str!("../resources/boxlang.json");

/// The directories created in a new home, as in the BoxLang runtime home.
const HOME_DIRS: [&str; 7] = [
    "bin", "classes", "config", "global", "logs", "lib", "modules",
];

/// The OS user's home directory (`HOME`, or `USERPROFILE` on Windows).
pub fn user_home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Pick the home directory. `flag` and `env_home` win over the `~/.matchbox` default.
///
/// # Errors
///
/// Returns [`io::ErrorKind::NotFound`] when no flag or `MATCHBOX_HOME` value is given and there is no user home to
/// put `.matchbox` in.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use matchbox_utility::home::resolve;
///
/// let user = Path::new("/home/me");
/// assert_eq!(resolve(Some("/flag"), Some("/env"), Some(user)).unwrap(), PathBuf::from("/flag"));
/// assert_eq!(resolve(None, Some("/env"), Some(user)).unwrap(), PathBuf::from("/env"));
/// assert_eq!(resolve(None, None, Some(user)).unwrap(), PathBuf::from("/home/me/.matchbox"));
/// ```
pub fn resolve(
    flag: Option<&str>,
    env_home: Option<&str>,
    user_home: Option<&Path>,
) -> io::Result<PathBuf> {
    if let Some(path) = flag.or(env_home).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    user_home.map(|home| home.join(".matchbox")).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot locate a user home directory; set MATCHBOX_HOME",
        )
    })
}

/// Path of the home's runtime config file.
pub fn config_path(home: &Path) -> PathBuf {
    home.join("config").join("boxlang.json")
}

/// Create the home layout and seed `config/boxlang.json` when it is missing.
///
/// Directories that already exist are left alone, and an existing config file is never overwritten.
///
/// Safe to call from several processes at once. The seed is written to a temporary file and then linked into place,
/// so another MatchBox that starts at the same moment sees either no config file or the complete one, never a
/// partly written file, and the process that loses the race leaves the winner's file alone.
///
/// # Errors
///
/// Returns the I/O error when a directory or the config file cannot be created.
pub fn ensure(home: &Path) -> io::Result<()> {
    for dir in HOME_DIRS {
        std::fs::create_dir_all(home.join(dir))?;
    }
    let config = config_path(home);
    if !config.exists() {
        seed(&config)?;
    }
    Ok(())
}

/// Write [`DEFAULT_CONFIG`] to `config` unless the file already exists, without ever exposing a partial file.
fn seed(config: &Path) -> io::Result<()> {
    let temporary = config.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, DEFAULT_CONFIG)?;
    // `hard_link` fails when `config` exists, so it never replaces a file another process (or the user) just wrote.
    let linked = std::fs::hard_link(&temporary, config);
    let _ = std::fs::remove_file(&temporary);
    match linked {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_beats_env_beats_default() {
        let user = Path::new("/u");
        assert_eq!(
            resolve(Some("/f"), Some("/e"), Some(user)).unwrap(),
            PathBuf::from("/f")
        );
        assert_eq!(
            resolve(None, Some("/e"), Some(user)).unwrap(),
            PathBuf::from("/e")
        );
        assert_eq!(
            resolve(None, None, Some(user)).unwrap(),
            PathBuf::from("/u/.matchbox")
        );
        assert_eq!(
            resolve(None, Some(""), Some(user)).unwrap(),
            PathBuf::from("/u/.matchbox")
        );
        assert!(resolve(None, None, None).is_err());
    }

    #[test]
    fn concurrent_first_runs_never_see_a_partial_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::sync::Arc::new(dir.path().join("home"));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let home = home.clone();
                std::thread::spawn(move || {
                    ensure(&home).unwrap();
                    // Every reader sees the complete file once ensure has returned, and none is ever empty.
                    assert_eq!(
                        std::fs::read_to_string(config_path(&home)).unwrap(),
                        DEFAULT_CONFIG
                    );
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let leftovers = std::fs::read_dir(home.join("config")).unwrap().count();
        assert_eq!(leftovers, 1, "temporary seed files are removed");
    }

    #[test]
    fn ensure_creates_layout_and_seeds_config_once() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        ensure(&home).unwrap();
        for sub in HOME_DIRS {
            assert!(home.join(sub).is_dir(), "{sub} missing");
        }
        let config = config_path(&home);
        assert_eq!(std::fs::read_to_string(&config).unwrap(), DEFAULT_CONFIG);

        std::fs::write(&config, "{ \"debugMode\": true }").unwrap();
        ensure(&home).unwrap();
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "{ \"debugMode\": true }"
        );
    }
}
