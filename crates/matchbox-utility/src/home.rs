//! The MatchBox home directory: `--bx-home`, then `MATCHBOX_HOME`, then `~/.matchbox`.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Default `config/boxlang.json` seeded into a new home.
pub const DEFAULT_CONFIG: &str = include_str!("../resources/boxlang.json");

const HOME_DIRS: [&str; 7] = [
    "bin", "classes", "config", "global", "logs", "lib", "modules",
];

/// The OS user's home directory (`HOME`, or `USERPROFILE` on Windows).
pub fn user_home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(|name| std::env::var_os(name))
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Pick the home directory. `flag` and `env_home` win over the `~/.matchbox` default.
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
pub fn ensure(home: &Path) -> io::Result<()> {
    for dir in HOME_DIRS {
        std::fs::create_dir_all(home.join(dir))?;
    }
    let config = config_path(home);
    if !config.exists() {
        std::fs::write(config, DEFAULT_CONFIG)?;
    }
    Ok(())
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
