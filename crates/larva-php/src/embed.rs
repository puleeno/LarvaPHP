//! Embedded PHP application support.
//!
//! Mirrors `embed.go`, `embed_unix.go`, and `embed_windows.go` from the Go source.

use std::path::Path;

/// Path to the embedded PHP application (empty if none).
pub static mut EMBEDDED_APP_PATH: Option<String> = None;

/// Set the embedded app path at build time.
pub fn set_embedded_app_path(path: &str) {
    unsafe {
        EMBEDDED_APP_PATH = Some(path.to_string());
    }
}

/// Get the embedded app path.
pub fn embedded_app_path() -> Option<&'static str> {
    unsafe {
        EMBEDDED_APP_PATH.as_deref()
    }
}

/// Remove the embedded app (call at process exit only).
pub fn remove_embedded_app() {
    if let Some(path) = embedded_app_path() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// Extract an embedded tar archive to a directory.
pub fn untar(embedded_data: &[u8], dir: &str) -> std::io::Result<()> {
    // TODO: implement tar extraction
    let _ = (embedded_data, dir);
    Ok(())
}

/// Prepare the embedded app directory (create, set permissions).
pub fn prepare_embedded_app_path(dir: &str) -> std::io::Result<()> {
    let path = Path::new(dir);
    if path.exists() {
        return Ok(());
    }

    // Create parent directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::create_dir(dir)?;
    Ok(())
}

/// Check if a path is local (not absolute, not a Windows UNC path).
pub fn is_local_path(path: &str) -> bool {
    !path.starts_with('/') && !path.starts_with('\\') && !path.contains(':')
}

/// Get the owner ID for the embedded app (Unix).
#[cfg(unix)]
pub fn embedded_app_owner_id() -> String {
    // SAFETY: geteuid is always safe to call
    unsafe { libc::geteuid().to_string() }
}

/// Get the owner ID for the embedded app (Windows).
#[cfg(windows)]
pub fn embedded_app_owner_id() -> String {
    // TODO: implement Windows SID
    "unknown".to_string()
}

/// Restrict embedded app path permissions.
#[cfg(unix)]
pub fn restrict_embedded_app_path(dir: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(dir)?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(dir, perms)
}

#[cfg(windows)]
pub fn restrict_embedded_app_path(_dir: &str) -> std::io::Result<()> {
    Ok(())
}

/// Check embedded app parent directory safety.
#[cfg(unix)]
pub fn check_embedded_app_parent(parent: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(parent)?;
    let perms = metadata.permissions();
    let mode = perms.mode();

    // Reject if writable by group or others without sticky bit
    if (mode & 0o022) != 0 && (mode & 0o1000) == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("embedded app parent directory {:?} must not be writable by group or others without the sticky bit", parent),
        ));
    }

    Ok(())
}

#[cfg(windows)]
pub fn check_embedded_app_parent(_parent: &str) -> std::io::Result<()> {
    Ok(())
}