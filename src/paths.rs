//! XDG Base Directory resolution and private-file persistence helpers.
//!
//! Two pieces live here:
//!
//! - [`AppDirs`] — scopes every XDG lookup to a single app name. Use
//!   `AppDirs::new("myapp")` and call `.config_dir()`, `.state_dir()`, etc.
//!   to get `$XDG_*/myapp/` paths.
//! - [`fs`] — file-system helpers that write files with `0o600` and create
//!   directories with `0o700` on Unix. Use for anything holding secrets.
//!
//! # Platforms
//!
//! - **Linux** (and other Unix but macOS): the XDG Base Directory spec,
//!   through the `dirs` crate.
//! - **macOS**: the XDG layout too, as the command-line tools there use it
//!   (git, gh, mise, fish, neovim, Homebrew): `$XDG_*_HOME` when set to an
//!   absolute path, else `~/.config`, `~/.local/share`, `~/.local/state`,
//!   `~/.cache`. Not Apple's `~/Library/Application Support`, which `dirs`
//!   gives for both config and data, so the two would be one directory, and
//!   which has no state directory at all. Before 0.10 railing used `dirs`
//!   here too: [`AppDirs::macos_library_dir`] is where that put an app's
//!   config and data, for an app moving them.
//!   The runtime directory falls back to `<cache>/<app>/run` (macOS has no
//!   `/run/user/<uid>`): [`AppDirs::runtime_dir`].
//! - **Windows**: the `dirs` crate's known folders; no runtime directory
//!   without `XDG_RUNTIME_DIR`.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::CliError;

// ---------------------------------------------------------------------------
// Base directories
// ---------------------------------------------------------------------------

/// The XDG base directories with a home default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Config,
    Data,
    State,
    Cache,
}

impl Base {
    fn name(self) -> &'static str {
        match self {
            Base::Config => "config",
            Base::Data => "data",
            Base::State => "state",
            Base::Cache => "cache",
        }
    }

    /// The variable that names it.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    fn var(self) -> &'static str {
        match self {
            Base::Config => "XDG_CONFIG_HOME",
            Base::Data => "XDG_DATA_HOME",
            Base::State => "XDG_STATE_HOME",
            Base::Cache => "XDG_CACHE_HOME",
        }
    }

    /// Where it is under the home directory when the variable is not set.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    fn under_home(self) -> &'static str {
        match self {
            Base::Config => ".config",
            Base::Data => ".local/share",
            Base::State => ".local/state",
            Base::Cache => ".cache",
        }
    }
}

/// An XDG variable's value, if it is usable: the spec ignores a relative
/// path (and an empty one), and so does this.
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The XDG spec's resolution of `base`, from `var` (the environment) and
/// `home`: the variable when absolute, else its place under home.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn xdg(
    base: Base,
    var: impl Fn(&str) -> Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    absolute(var(base.var())).or_else(|| Some(home?.join(base.under_home())))
}

/// `base` on this platform (see the module's "Platforms").
fn base_dir(base: Base) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        xdg(base, |k| std::env::var_os(k), dirs::home_dir())
    }
    #[cfg(not(target_os = "macos"))]
    {
        match base {
            Base::Config => dirs::config_dir(),
            Base::Data => dirs::data_dir(),
            Base::State => dirs::state_dir(),
            Base::Cache => dirs::cache_dir(),
        }
    }
}

/// The runtime directory for `app`: `$XDG_RUNTIME_DIR/<app>` when that is
/// set to an absolute path, else `fallback` (macOS's `<cache>/<app>/run`).
fn runtime(
    app: &str,
    var: impl Fn(&str) -> Option<OsString>,
    fallback: Option<PathBuf>,
) -> Option<PathBuf> {
    absolute(var("XDG_RUNTIME_DIR"))
        .map(|base| base.join(app))
        .or(fallback)
}

// ---------------------------------------------------------------------------
// AppDirs
// ---------------------------------------------------------------------------

/// Scoped XDG base directory resolver.
///
/// Every method but [`AppDirs::create_runtime_dir`] returns `$XDG_*/<app>/`
/// (or the platform's equivalent, see the module's "Platforms") without
/// touching the filesystem. The caller is responsible for creating the
/// directory if it doesn't yet exist — typically with
/// [`fs::create_private_dir_all`] when the contents are sensitive.
pub struct AppDirs {
    pub app: &'static str,
}

impl AppDirs {
    pub const fn new(app: &'static str) -> Self {
        Self { app }
    }

    fn base(&self, base: Base) -> Result<PathBuf, CliError> {
        Ok(base_dir(base)
            .ok_or_else(|| {
                CliError::internal(format!("unable to determine XDG {} directory", base.name()))
            })?
            .join(self.app))
    }

    /// `$XDG_CONFIG_HOME/<app>` (typically `~/.config/<app>`, macOS too).
    pub fn config_dir(&self) -> Result<PathBuf, CliError> {
        self.base(Base::Config)
    }

    /// `$XDG_STATE_HOME/<app>` (typically `~/.local/state/<app>`, macOS too).
    pub fn state_dir(&self) -> Result<PathBuf, CliError> {
        self.base(Base::State)
    }

    /// `$XDG_DATA_HOME/<app>` (typically `~/.local/share/<app>`, macOS too).
    pub fn data_dir(&self) -> Result<PathBuf, CliError> {
        self.base(Base::Data)
    }

    /// `$XDG_CACHE_HOME/<app>` (typically `~/.cache/<app>`, macOS too).
    pub fn cache_dir(&self) -> Result<PathBuf, CliError> {
        self.base(Base::Cache)
    }

    /// `$XDG_RUNTIME_DIR/<app>`, for sockets, locks and other files that
    /// belong to a running process.
    ///
    /// Never a shared `/tmp` path. Where the system provides no
    /// `XDG_RUNTIME_DIR`:
    ///
    /// - **macOS**, which has no `/run/user/<uid>`: `<cache_dir>/run`
    ///   (`~/.cache/<app>/run`), in the user's home, short enough for a
    ///   socket's 104-byte path. Unlike `/run/user` it is on disk and is
    ///   not emptied at logout or boot, and nothing makes it private on its
    ///   own: create it with [`AppDirs::create_runtime_dir`], which does.
    ///   Not `$TMPDIR`: macOS deletes what is unused there for three days,
    ///   a long-running agent's socket included.
    /// - **elsewhere**: an error. Linux sessions always set it, and a missing
    ///   one is a broken session, not a reason to fall back.
    pub fn runtime_dir(&self) -> Result<PathBuf, CliError> {
        let fallback = if cfg!(target_os = "macos") {
            self.cache_dir().ok().map(|cache| cache.join("run"))
        } else {
            None
        };
        runtime(self.app, |k| std::env::var_os(k), fallback).ok_or_else(|| {
            CliError::internal(if cfg!(target_os = "macos") {
                "XDG_RUNTIME_DIR not set and no home directory for the fallback"
            } else {
                "XDG_RUNTIME_DIR not set — required for secure runtime paths"
            })
        })
    }

    /// [`AppDirs::runtime_dir`], created if missing (`0o700`, its parents
    /// as [`fs::create_private_dir_all`] makes them), and checked: a real
    /// directory, not a link, owned by this user and closed to everyone
    /// else. Anything else is refused rather than repaired, as `ssh` and
    /// `gpg` refuse a socket directory: it may not be this user's to change.
    pub fn create_runtime_dir(&self) -> Result<PathBuf, CliError> {
        let dir = self.runtime_dir()?;
        fs::create_private_dir_all(&dir).map_err(CliError::internal)?;
        fs::check_private_dir(&dir).map_err(CliError::internal)?;
        Ok(dir)
    }

    /// macOS only: where railing before 0.10 put this app's config *and*
    /// data, `~/Library/Application Support/<app>` (the `dirs` crate's
    /// choice), so an app can find what it wrote there and move it. `None`
    /// on every other platform, where nothing moved.
    pub fn macos_library_dir(&self) -> Option<PathBuf> {
        if cfg!(target_os = "macos") {
            Some(
                dirs::home_dir()?
                    .join("Library/Application Support")
                    .join(self.app),
            )
        } else {
            None
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// An environment of `pairs`, never the process's own.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| {
            pairs
                .iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from("/Users/ola"))
    }

    #[test]
    fn with_no_variables_each_base_is_its_place_under_home() {
        let none = env(&[]);
        let at = |b| xdg(b, &none, home()).unwrap();
        assert_eq!(at(Base::Config), Path::new("/Users/ola/.config"));
        assert_eq!(at(Base::Data), Path::new("/Users/ola/.local/share"));
        assert_eq!(at(Base::State), Path::new("/Users/ola/.local/state"));
        assert_eq!(at(Base::Cache), Path::new("/Users/ola/.cache"));
    }

    /// The bug this layout fixes on macOS: `dirs` gives one directory for
    /// both, so an app's config sat inside its data.
    #[test]
    fn config_and_data_are_never_one_directory() {
        let none = env(&[]);
        for b in [Base::Config, Base::State, Base::Cache] {
            assert_ne!(
                xdg(b, &none, home()),
                xdg(Base::Data, &none, home()),
                "{b:?}"
            );
        }
    }

    #[test]
    fn an_absolute_variable_wins() {
        let set = env(&[("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(xdg(Base::Config, &set, home()).unwrap(), Path::new("/cfg"));
        assert_eq!(xdg(Base::Data, &set, home()).unwrap(), Path::new("/data"));
        assert_eq!(
            xdg(Base::Cache, &set, home()).unwrap(),
            Path::new("/Users/ola/.cache")
        );
    }

    /// The spec: a relative or empty value is ignored, not joined to the
    /// working directory.
    #[test]
    fn a_relative_or_empty_variable_is_ignored() {
        for value in ["", "relative/cfg", "./cfg"] {
            let set = env(&[("XDG_CONFIG_HOME", value)]);
            assert_eq!(
                xdg(Base::Config, &set, home()).unwrap(),
                Path::new("/Users/ola/.config"),
                "{value:?}"
            );
        }
    }

    #[test]
    fn with_no_home_and_no_variable_there_is_none() {
        assert_eq!(xdg(Base::Config, env(&[]), None), None);
    }

    #[test]
    fn the_runtime_dir_is_the_variables_or_the_fallback() {
        let fallback = Some(PathBuf::from("/Users/ola/.cache/hush/run"));
        let set = env(&[("XDG_RUNTIME_DIR", "/run/user/501")]);
        assert_eq!(
            runtime("hush", &set, fallback.clone()).unwrap(),
            Path::new("/run/user/501/hush")
        );
        assert_eq!(runtime("hush", env(&[]), fallback.clone()), fallback);
        // A relative one is as good as none: never a socket under the
        // working directory.
        let rel = env(&[("XDG_RUNTIME_DIR", "run")]);
        assert_eq!(runtime("hush", &rel, fallback.clone()), fallback);
        assert_eq!(runtime("hush", &rel, None), None);
    }

    /// The fallback's sockets fit macOS's 104-byte `sun_path` (103 and the
    /// NUL) with a home of a usual length.
    #[test]
    fn a_fallback_socket_path_fits_macos() {
        let run = xdg(
            Base::Cache,
            env(&[]),
            Some(PathBuf::from("/Users/a-rather-long-user-name")),
        )
        .unwrap()
        .join("some-app/run/some-app-browser.sock");
        assert!(
            run.as_os_str().len() < 104,
            "{} bytes",
            run.as_os_str().len()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_old_place_is_apples_library() {
        let old = AppDirs::new("x").macos_library_dir().unwrap();
        assert!(
            old.ends_with("Library/Application Support/x"),
            "{}",
            old.display()
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn elsewhere_nothing_moved() {
        assert_eq!(AppDirs::new("x").macos_library_dir(), None);
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
    use std::path::Path;

    use crate::error::{Context, WithContext};

    /// What these return: the failure, saying which file and what was being
    /// done to it, with the I/O error as its source.
    pub type Result<T> = std::result::Result<T, WithContext>;

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
            .context(|| format!("Failed to open file for writing: {}", path.display()))?;

        file.write_all(data)
            .context(|| format!("Failed to write file: {}", path.display()))?;

        Ok(())
    }

    #[cfg(not(unix))]
    fn _write_private_file(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data).context(|| format!("Failed to write file: {}", path.display()))
    }

    /// Write `data` to `path` atomically, with owner-only permissions
    /// (`0o600` on Unix).
    ///
    /// Unlike [`write_private_file`], a reader never observes a truncated or
    /// partially written file: the data goes to a sibling temporary file which
    /// is then renamed onto `path`. Rename within a directory is atomic on
    /// POSIX, so `path` always holds either the complete previous contents or
    /// the complete new ones.
    ///
    /// Use this for state files that are re-read on the next run — a crash or
    /// `^C` midway through a plain write can otherwise leave behind a file that
    /// no longer parses.
    ///
    /// The temporary file is created with the target permissions *before* any
    /// bytes are written, so the contents are never briefly readable at the
    /// process umask. It is removed on failure.
    pub fn write_private_file_atomic(path: &Path, data: impl AsRef<[u8]>) -> Result<()> {
        _write_private_file_atomic(path, data.as_ref())
    }

    #[cfg(unix)]
    fn _write_private_file_atomic(path: &Path, data: &[u8]) -> Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let tmp_path = temp_sibling(path);

        let write_tmp = || -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)
                .context(|| format!("Failed to create temporary file: {}", tmp_path.display()))?;

            file.write_all(data)
                .context(|| format!("Failed to write file: {}", tmp_path.display()))?;

            // Flush to disk before the rename so a crash cannot leave the
            // renamed file present but empty.
            file.sync_all()
                .context(|| format!("Failed to flush file: {}", tmp_path.display()))
        };

        if let Err(e) = write_tmp() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        std::fs::rename(&tmp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            WithContext::new(format!("Failed to replace file: {}", path.display()), e)
        })
    }

    #[cfg(not(unix))]
    fn _write_private_file_atomic(path: &Path, data: &[u8]) -> Result<()> {
        let tmp_path = temp_sibling(path);

        if let Err(e) = std::fs::write(&tmp_path, data)
            .context(|| format!("Failed to write file: {}", tmp_path.display()))
        {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        // Windows `rename` fails when the destination exists.
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            WithContext::new(format!("Failed to replace file: {}", path.display()), e)
        })
    }

    /// Build the temporary path used by [`write_private_file_atomic`].
    ///
    /// It must live in the same directory as the target, otherwise the rename
    /// could cross a filesystem boundary and stop being atomic. The PID keeps
    /// concurrent writers from clobbering each other's temporaries.
    fn temp_sibling(path: &Path) -> std::path::PathBuf {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".tmp.{}", std::process::id()));
        path.with_file_name(name)
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
            .context(|| format!("Failed to create directory: {}", dir.display()))
    }

    #[cfg(not(unix))]
    fn _create_private_dir_all(dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .context(|| format!("Failed to create directory: {}", dir.display()))
    }

    /// Check that `dir` is a directory private to this user: not a symbolic
    /// link, owned by the effective user, and with no permission for group
    /// or others. The error says which of these failed. Always `Ok` on
    /// non-Unix, where access is the ACLs'.
    pub fn check_private_dir(dir: &Path) -> Result<()> {
        _check_private_dir(dir)
    }

    #[cfg(unix)]
    fn _check_private_dir(dir: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        let refuse = |why: String| {
            Err(WithContext::new(
                format!("Refusing directory: {}", dir.display()),
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, why),
            ))
        };
        // Not followed: a link is refused, not trusted for its target.
        let meta = std::fs::symlink_metadata(dir)
            .context(|| format!("Failed to read metadata for: {}", dir.display()))?;
        if meta.file_type().is_symlink() {
            return refuse("it is a symbolic link".into());
        }
        if !meta.is_dir() {
            return refuse("it is not a directory".into());
        }
        let me = rustix::process::geteuid().as_raw();
        if meta.uid() != me {
            return refuse(format!("it is owned by uid {}, not {me}", meta.uid()));
        }
        let mode = meta.mode() & 0o777;
        if mode & 0o077 != 0 {
            return refuse(format!(
                "its mode is {mode:o}, open to others (chmod 700 \"{}\")",
                dir.display()
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn _check_private_dir(_dir: &Path) -> Result<()> {
        Ok(())
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
            .context(|| format!("Failed to read metadata for: {}", path.display()))?;
        let mode = meta.permissions().mode() & 0o777;

        let expected = if meta.is_dir() { 0o700 } else { 0o600 };
        Ok(mode == expected)
    }

    #[cfg(not(unix))]
    fn _check_private_permissions(_path: &Path) -> Result<bool> {
        Ok(true)
    }

    /// The warning for `path` when its permissions are broader than private
    /// (`0700` for a directory, `0600` for a file), with the `chmod` that
    /// fixes it; `None` when they are private or cannot be read. For an app
    /// that shows it in its own frame.
    pub fn broad_permissions_warning(path: &Path) -> Option<String> {
        if check_private_permissions(path).ok()? {
            return None;
        }
        let mode = if path.is_dir() { "700" } else { "600" };
        Some(format!(
            "'{}' has overly broad permissions. Run: chmod {mode} \"{}\"",
            path.display(),
            path.display()
        ))
    }

    /// Emit a warning to stderr if `path` has overly broad permissions.
    pub fn warn_if_permissions_broad(path: &Path) {
        if let Some(w) = broad_permissions_warning(path) {
            eprintln!("warning: {w}");
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
        fn test_write_private_file_atomic_creates_file() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file_atomic(&path, b"{\"test\": true}").unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert_eq!(content, "{\"test\": true}");
        }

        #[test]
        #[cfg(unix)]
        fn test_write_private_file_atomic_mode_600() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file_atomic(&path, b"data").unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "expected mode 0o600, got 0o{:o}", mode);
        }

        #[test]
        fn test_write_private_file_atomic_replaces_existing() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file_atomic(&path, b"old contents, much longer").unwrap();
            write_private_file_atomic(&path, b"new").unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        }

        /// The temporary file must not survive a successful write — a stray
        /// `.tmp.<pid>` sibling would be mistaken for state by directory walks
        /// such as `ksef auth purge`.
        #[test]
        fn test_write_private_file_atomic_leaves_no_temp_file() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file_atomic(&path, b"data").unwrap();

            let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n != "secret.json")
                .collect();
            assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");
        }

        /// A failed write must leave the previous contents intact — that is the
        /// whole point of going through a temporary file.
        #[test]
        fn test_write_private_file_atomic_failure_preserves_original() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("secret.json");
            write_private_file_atomic(&path, b"original").unwrap();

            // A directory in place of the temp file makes create_new fail.
            let tmp_path = temp_sibling(&path);
            std::fs::create_dir(&tmp_path).unwrap();

            assert!(write_private_file_atomic(&path, b"replacement").is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        }

        #[test]
        fn test_temp_sibling_stays_in_same_directory() {
            let path = Path::new("/some/dir/tokens.json");
            let tmp_path = temp_sibling(path);
            assert_eq!(tmp_path.parent(), path.parent());
            assert!(
                tmp_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("tokens.json.tmp.")
            );
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
        fn a_private_dir_passes_and_an_open_one_is_refused() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = TempDir::new().unwrap();
            let dir = tmp.path().join("run");
            create_private_dir_all(&dir).unwrap();
            check_private_dir(&dir).unwrap();

            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let err = check_private_dir(&dir).unwrap_err();
            let why = std::error::Error::source(&err).unwrap().to_string();
            assert!(why.contains("755"), "{why}");
        }

        #[test]
        #[cfg(unix)]
        fn a_link_to_a_private_dir_is_refused() {
            let tmp = TempDir::new().unwrap();
            let real = tmp.path().join("real");
            create_private_dir_all(&real).unwrap();
            let link = tmp.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let err = check_private_dir(&link).unwrap_err();
            let why = std::error::Error::source(&err).unwrap().to_string();
            assert!(why.contains("symbolic link"), "{why}");
        }

        #[test]
        #[cfg(unix)]
        fn a_file_is_not_a_private_dir() {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("f");
            write_private_file(&path, b"x").unwrap();
            let why = std::error::Error::source(&check_private_dir(&path).unwrap_err())
                .unwrap()
                .to_string();
            assert!(why.contains("not a directory"), "{why}");
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
