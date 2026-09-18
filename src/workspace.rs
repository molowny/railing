//! Project-local workspace config discovery.
//!
//! CLIs often want a project-local TOML file (`.hackbox.toml`, `.ksef.toml`,
//! etc.) that lives somewhere in the project tree and overrides or augments
//! global XDG config. This module provides a single generic helper that
//! walks ancestor directories for a named file and parses it into any
//! `serde::Deserialize` type.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

/// Walk upward from `start_dir` looking for `filename`. Returns the parsed
/// config and the directory that contained the file — callers use that dir
/// to resolve relative paths declared inside the config.
///
/// A malformed file is surfaced as an `Err`, not silently skipped: if the
/// user put a file there, they meant for it to take effect.
pub fn find<T: DeserializeOwned>(filename: &str, start_dir: &Path) -> Result<Option<(T, PathBuf)>> {
    let mut current = start_dir;
    loop {
        let candidate = current.join(filename);
        if candidate.is_file() {
            let content = fs::read_to_string(&candidate)
                .with_context(|| format!("failed to read {}", candidate.display()))?;
            let cfg: T = toml::from_str(&content)
                .with_context(|| format!("failed to parse {}", candidate.display()))?;
            return Ok(Some((cfg, current.to_path_buf())));
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Ok(None),
        }
    }
}

/// Resolve a path that may be relative to the config file's directory.
/// Absolute paths are returned unchanged.
pub fn resolve_relative(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::fs;
    use tempfile::TempDir;

    #[derive(Debug, Deserialize, Default, PartialEq)]
    #[serde(default)]
    struct Sample {
        name: Option<String>,
    }

    #[test]
    fn finds_file_in_ancestor() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("a/b/c");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(tmp.path().join(".sample.toml"), r#"name = "found""#).unwrap();

        let (cfg, dir) = find::<Sample>(".sample.toml", &subdir).unwrap().unwrap();
        assert_eq!(cfg.name.as_deref(), Some("found"));
        assert_eq!(dir, tmp.path());
    }

    #[test]
    fn returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("deep/nested");
        fs::create_dir_all(&subdir).unwrap();
        assert!(find::<Sample>(".sample.toml", &subdir).unwrap().is_none());
    }

    #[test]
    fn prefers_nearest_ancestor() {
        let tmp = TempDir::new().unwrap();
        let mid = tmp.path().join("a");
        let subdir = mid.join("b");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(tmp.path().join(".sample.toml"), r#"name = "root""#).unwrap();
        fs::write(mid.join(".sample.toml"), r#"name = "mid""#).unwrap();

        let (cfg, dir) = find::<Sample>(".sample.toml", &subdir).unwrap().unwrap();
        assert_eq!(cfg.name.as_deref(), Some("mid"));
        assert_eq!(dir, mid);
    }

    #[test]
    fn malformed_file_is_error() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".sample.toml"), "this = is = broken").unwrap();
        assert!(find::<Sample>(".sample.toml", tmp.path()).is_err());
    }

    #[test]
    fn resolve_relative_joins_relative() {
        let base = Path::new("/project/root");
        assert_eq!(
            resolve_relative(base, Path::new("tmpl")),
            PathBuf::from("/project/root/tmpl")
        );
    }

    #[test]
    fn resolve_relative_passes_absolute() {
        let base = Path::new("/ignored");
        assert_eq!(
            resolve_relative(base, Path::new("/abs/path")),
            PathBuf::from("/abs/path")
        );
    }
}
