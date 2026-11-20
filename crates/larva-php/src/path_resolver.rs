//! OS-aware path resolution using the Strategy pattern.
//!
//! Different operating systems have different path quirks:
//! - **Windows**: `std::fs::canonicalize` returns a verbatim `\\?\C:\...` path
//!   that Rust understands but many Win32 programs (e.g. php-cgi.exe) do not.
//! - **Unix**: canonicalize returns a plain absolute path.
//!
//! Rather than sprinkling `#[cfg(windows)]` throughout the code, this module
//! defines a [`PathResolver`] strategy with OS-specific implementations, and a
//! [`default_resolver`] factory that picks the right one for the running OS.

use std::io;
use std::path::{Path, PathBuf};

/// Strategy: resolve and normalize filesystem paths for the current OS.
pub trait PathResolver: Send + Sync {
    /// Human-readable resolver name (for diagnostics/logging).
    fn name(&self) -> &'static str;

    /// Resolve a possibly-relative root to a canonical absolute path.
    ///
    /// Falls back to the plain absolute path when canonicalization fails
    /// (e.g. the path does not exist yet).
    fn resolve_root(&self, root: &str) -> io::Result<PathBuf>;

    /// Convert a path into the form expected by external (non-Rust) programs.
    ///
    /// On Windows this strips the `\\?\` verbatim prefix so Win32 programs like
    /// php-cgi can open the path. On Unix this is a no-op.
    fn make_native(&self, path: &Path) -> PathBuf;
}

/// Windows resolver: strips the `\\?\` verbatim prefix produced by
/// `std::fs::canonicalize`.
pub struct WindowsPathResolver;

impl PathResolver for WindowsPathResolver {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn resolve_root(&self, root: &str) -> io::Result<PathBuf> {
        let abs = crate::cgi::fastabs::fast_abs(root)?;
        let canonical = match std::fs::canonicalize(&abs) {
            Ok(c) => c,
            Err(_) => abs,
        };
        Ok(self.make_native(&canonical))
    }

    fn make_native(&self, path: &Path) -> PathBuf {
        let s = path.to_string_lossy();
        // `\\?\UNC\server\share` -> `\\server\share`
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        // `\\?\C:\path` -> `C:\path`
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        path.to_path_buf()
    }
}

/// Unix resolver: canonicalize returns a plain absolute path; no stripping.
pub struct UnixPathResolver;

impl PathResolver for UnixPathResolver {
    fn name(&self) -> &'static str {
        "unix"
    }

    fn resolve_root(&self, root: &str) -> io::Result<PathBuf> {
        let abs = crate::cgi::fastabs::fast_abs(root)?;
        match std::fs::canonicalize(&abs) {
            Ok(c) => Ok(c),
            Err(_) => Ok(abs),
        }
    }

    fn make_native(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
}

/// Factory: return the [`PathResolver`] appropriate for the running OS.
pub fn default_resolver() -> Box<dyn PathResolver> {
    if cfg!(windows) {
        Box::new(WindowsPathResolver)
    } else {
        Box::new(UnixPathResolver)
    }
}

/// Convenience: resolve a root path using the OS resolver (no allocation).
pub fn resolve_root(root: &str) -> io::Result<PathBuf> {
    if cfg!(windows) {
        WindowsPathResolver.resolve_root(root)
    } else {
        UnixPathResolver.resolve_root(root)
    }
}

/// Convenience: make a path native (strip verbatim prefix on Windows).
/// Uses the OS resolver directly so the hot path does no heap allocation.
pub fn make_native(path: &Path) -> PathBuf {
    if cfg!(windows) {
        WindowsPathResolver.make_native(path)
    } else {
        UnixPathResolver.make_native(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolver_has_a_name() {
        let r = default_resolver();
        assert!(!r.name().is_empty());
    }

    #[test]
    fn resolve_root_existing_dir() {
        let dir = std::env::temp_dir();
        let resolved = resolve_root(&dir.to_string_lossy()).unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.exists());
    }

    #[test]
    fn resolve_root_nonexistent_falls_back() {
        let resolved = resolve_root("/nonexistent/path/xyz-123").unwrap();
        assert!(resolved.to_string_lossy().contains("xyz-123"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_make_native_strips_verbatim() {
        let r = WindowsPathResolver;
        let verbatim = PathBuf::from(r"\\?\C:\www\index.php");
        let native = r.make_native(&verbatim);
        assert_eq!(native, PathBuf::from(r"C:\www\index.php"));

        let unc = PathBuf::from(r"\\?\UNC\server\share\file");
        let native_unc = r.make_native(&unc);
        assert_eq!(native_unc, PathBuf::from(r"\\server\share\file"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_make_native_no_prefix_is_identity() {
        let r = WindowsPathResolver;
        let plain = PathBuf::from(r"C:\www\index.php");
        assert_eq!(r.make_native(&plain), plain);
    }

    #[cfg(unix)]
    #[test]
    fn unix_make_native_is_identity() {
        let r = UnixPathResolver;
        let p = PathBuf::from("/var/www/index.php");
        assert_eq!(r.make_native(&p), p);
    }
}