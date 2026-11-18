//! `.env` file loading, mirroring the BoxLang CLI: a user-level file first, then a project-level
//! file. Project values win over user values, and file values win over the OS environment.
//!
//! Format: `KEY=VALUE`, one per line. Lines starting with `#` are comments. Quotes are optional.

use std::{collections::BTreeMap, io, path::Path};

/// Parse `.env` content into ordered `(key, value)` pairs.
///
/// # Examples
///
/// ```
/// use matchbox_utility::env_file::parse;
///
/// let vars = parse("# a comment\nKEY=value\nexport QUOTED=\"two words\"\n");
/// assert_eq!(
///     vars,
///     vec![
///         ("KEY".to_string(), "value".to_string()),
///         ("QUOTED".to_string(), "two words".to_string()),
///     ]
/// );
/// ```
pub fn parse(content: &str) -> Vec<(String, String)> {
    content.lines().filter_map(parse_line).collect()
}

/// One `.env` line as a `(key, value)` pair, or `None` for a blank line, a comment, a line without `=`, or a key or
/// value that the process environment cannot hold (an empty key, `=` in the key, or a NUL byte).
fn parse_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").map_or(line, str::trim_start);
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
        return None;
    }
    Some((key.to_string(), unquote(value.trim()).to_string()))
}

/// Strip one pair of matching single or double quotes around a value. Nothing inside is unescaped.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// Read one file. A missing file is not an error.
///
/// # Errors
///
/// Returns the I/O error when the file exists but cannot be read.
pub fn read(path: &Path) -> io::Result<Vec<(String, String)>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse(&content)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Merge the user-level and project-level files. Project values override user values.
///
/// # Errors
///
/// Returns the I/O error when either file exists but cannot be read.
pub fn load(user_file: Option<&Path>, project_file: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    if let Some(user_file) = user_file {
        vars.extend(read(user_file)?);
    }
    vars.extend(read(project_file)?);
    Ok(vars)
}

/// Set the variables in the process environment so `getSystemSetting()` and child processes see them.
///
/// Call once at startup, before any other thread reads or writes the environment.
pub fn apply(vars: &BTreeMap<String, String>) {
    for (key, value) in vars {
        // SAFETY: documented above, called from single-threaded startup.
        unsafe { std::env::set_var(key, value) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_comments_quotes_and_export() {
        let vars = parse(
            "# comment\n\nA=1\n B = two \nC=\"quoted value\"\nD='single'\nexport E=5\nBAD LINE\n=novalue\nF=a=b\n",
        );
        assert_eq!(
            vars,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two".into()),
                ("C".into(), "quoted value".into()),
                ("D".into(), "single".into()),
                ("E".into(), "5".into()),
                ("F".into(), "a=b".into()),
            ]
        );
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join(".env")).unwrap().is_empty());
    }

    #[test]
    fn project_overrides_user() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.env");
        let project = dir.path().join(".env");
        std::fs::write(&user, "DB_HOST=localhost\nDB_PORT=5432\n").unwrap();
        std::fs::write(&project, "DB_HOST=staging\nAPP_ENV=staging\n").unwrap();
        let vars = load(Some(&user), &project).unwrap();
        assert_eq!(vars["DB_HOST"], "staging");
        assert_eq!(vars["DB_PORT"], "5432");
        assert_eq!(vars["APP_ENV"], "staging");
    }

    #[test]
    fn file_values_override_os_environment() {
        let key = "MATCHBOX_UTILITY_TEST_ENV_OVERRIDE";
        unsafe { std::env::set_var(key, "os") };
        let mut vars = BTreeMap::new();
        vars.insert(key.to_string(), "file".to_string());
        apply(&vars);
        assert_eq!(std::env::var(key).unwrap(), "file");
    }
}
