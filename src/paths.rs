//! XDG Base Directory resolution and private-file persistence helpers.
//!
//! Two pieces live here:
//!
//! - [`AppDirs`] — thin wrapper over the `dirs` crate that scopes every XDG
//!   lookup to a single app name. Use `AppDirs::new("myapp")` and call
//!   `.config_dir()`, `.state_dir()`, etc. to get `$XDG_*/myapp/` paths.
//! - [`fs`] — file-system helpers that write files with `0o600` and create
//!   directories with `0o700` on Unix. Use for anything holding secrets.

use std::path::PathBuf;

use crate::CliError;

// ---------------------------------------------------------------------------
// AppDirs
// ---------------------------------------------------------------------------

/// Scoped XDG base directory resolver.
///
/// Every method returns `$XDG_*/<app>/` (or the equivalent platform path)
/// without touching the filesystem. The caller is responsible for creating
/// the directory if it doesn't yet exist — typically with
/// [`fs::create_private_dir_all`] when the contents are sensitive.
pub struct AppDirs {
    pub app: &'static str,
}

impl AppDirs {
    pub const fn new(app: &'static str) -> Self {
        Self { app }
    }

    /// `$XDG_CONFIG_HOME/<app>` (typically `~/.config/<app>`).
    pub fn config_dir(&self) -> Result<PathBuf, CliError> {
        Ok(dirs::config_dir()
            .ok_or_else(|| {
                CliError::Internal(anyhow::anyhow!("unable to determine XDG config directory"))
            })?
            .join(self.app))
    }

    /// `$XDG_STATE_HOME/<app>` (typically `~/.local/state/<app>`).
    pub fn state_dir(&self) -> Result<PathBuf, CliError> {
        Ok(dirs::state_dir()
            .ok_or_else(|| {
                CliError::Internal(anyhow::anyhow!("unable to determine XDG state directory"))
            })?
            .join(self.app))
    }

    /// `$XDG_DATA_HOME/<app>` (typically `~/.local/share/<app>`).
    pub fn data_dir(&self) -> Result<PathBuf, CliError> {
        Ok(dirs::data_dir()
            .ok_or_else(|| {
                CliError::Internal(anyhow::anyhow!("unable to determine XDG data directory"))
            })?
            .join(self.app))
    }

    /// `$XDG_CACHE_HOME/<app>` (typically `~/.cache/<app>`).
    pub fn cache_dir(&self) -> Result<PathBuf, CliError> {
        Ok(dirs::cache_dir()
            .ok_or_else(|| {
                CliError::Internal(anyhow::anyhow!("unable to determine XDG cache directory"))
            })?
            .join(self.app))
    }

    /// `$XDG_RUNTIME_DIR/<app>`.
    ///
    /// Unlike the other dirs this intentionally has no fallback — if
    /// `XDG_RUNTIME_DIR` is unset the caller almost always wants a hard
    /// error rather than a world-readable `/tmp` path (this is how
    /// sealward's agent socket enforces secure IPC).
    pub fn runtime_dir(&self) -> Result<PathBuf, CliError> {
        let base = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "XDG_RUNTIME_DIR not set — required for secure runtime paths"
            ))
        })?;
        Ok(PathBuf::from(base).join(self.app))
    }

    /// Shortcut for `config_dir().join(name)`.
    pub fn config_file(&self, name: &str) -> Result<PathBuf, CliError> {
        Ok(self.config_dir()?.join(name))
    }

    /// Shortcut for `state_dir().join(name)`.
    pub fn state_file(&self, name: &str) -> Result<PathBuf, CliError> {
        Ok(self.state_dir()?.join(name))
    }
}

// ---------------------------------------------------------------------------
// fs — private file helpers
// ---------------------------------------------------------------------------

/// File-system utilities for private state file management.
///
/// Writes files with `0o600` and creates directories with `0o700` on Unix.
/// On non-Unix platforms these fall back to the standard library equivalents
/// (Windows manages access through ACLs, not mode bits).
pub mod fs {
    use anyhow::{Context, Result};
    use std::path::Path;

    /// Write `data` to `path` with owner-only permissions (`0o600` on Unix).
    pub fn write_private_file(path: &Path, data: impl AsRef<[u8]>) -> Result<()> {
        _write_private_file(path, data.as_ref())
    }

    #[cfg(unix)]
    fn _write_private_file(path: &Path, data: &[u8]) -> Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("Failed to open file for writing: {}", path.display()))?;

        file.write_all(data)
            .with_context(|| format!("Failed to write file: {}", path.display()))?;

        Ok(())
    }

    #[cfg(not(unix))]
    fn _write_private_file(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)
            .with_context(|| format!("Failed to write file: {}", path.display()))
    }

    /// Create `dir` and all parents with owner-only permissions
    /// (`0o700` on Unix). Idempotent.
    pub fn create_private_dir_all(dir: &Path) -> Result<()> {
        _create_private_dir_all(dir)
    }

    #[cfg(unix)]
    fn _create_private_dir_all(dir: &Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))
    }

    #[cfg(not(unix))]
    fn _create_private_dir_all(dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))
    }

    /// Check whether `path` has owner-only permissions (`0o600` for files,
    /// `0o700` for directories). Always `Ok(true)` on non-Unix.
    pub fn check_private_permissions(path: &Path) -> Result<bool> {
        _check_private_permissions(path)
    }

    #[cfg(unix)]
    fn _check_private_permissions(path: &Path) -> Result<bool> {
        use std::os::unix::fs::PermissionsExt;

        let meta = std::fs::metadata(path)
            .with_context(|| format!("Failed to read metadata for: {}", path.display()))?;
        let mode = meta.permissions().mode() & 0o777;

        let expected = if meta.is_dir() { 0o700 } else { 0o600 };
        Ok(mode == expected)
    }

    #[cfg(not(unix))]
    fn _check_private_permissions(_path: &Path) -> Result<bool> {
        Ok(true)
    }

    /// Emit a warning to stderr if `path` has overly broad permissions.
    pub fn warn_if_permissions_broad(path: &Path) {
        match check_private_permissions(path) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!(
                    "warning: '{}' has overly broad permissions. \
                     Run: chmod 600 \"{}\"",
                    path.display(),
                    path.display()
                );
            }
            Err(_) => {}
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        #[test]
        fn test_write_private_file_creates_file() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file(&path, b"{\"test\": true}").unwrap();
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).unwrap();
            assert_eq!(content, "{\"test\": true}");
        }

        #[test]
        #[cfg(unix)]
        fn test_write_private_file_mode_600() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file(&path, b"data").unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "expected mode 0o600, got 0o{:o}", mode);
        }

        #[test]
        #[cfg(unix)]
        fn test_create_private_dir_mode_700() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = TempDir::new().unwrap();
            let dir = tmp
                .path()
                .join("profiles")
                .join("default")
                .join("envs")
                .join("test");
            create_private_dir_all(&dir).unwrap();
            assert!(dir.exists());
            let meta = std::fs::metadata(&dir).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "expected mode 0o700, got 0o{:o}", mode);
        }

        #[test]
        fn test_create_private_dir_idempotent() {
            let tmp = TempDir::new().unwrap();
            let dir = tmp.path().join("mydir");
            create_private_dir_all(&dir).unwrap();
            create_private_dir_all(&dir).unwrap();
        }

        #[test]
        #[cfg(unix)]
        fn test_check_private_permissions_correct() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("f");
            write_private_file(&path, b"x").unwrap();
            assert!(check_private_permissions(&path).unwrap());
        }

        #[test]
        #[cfg(unix)]
        fn test_check_private_permissions_too_broad() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("f");
            std::fs::write(&path, b"x").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!check_private_permissions(&path).unwrap());
        }
    }
}
