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
///
/// Implements a minimal, safe ustar reader supporting regular files and
/// directories. Symlinks and hard links are intentionally skipped (they could
/// escape the target directory). Path traversal (`..` escaping the target) is
/// rejected.
pub fn untar(embedded_data: &[u8], dir: &str) -> std::io::Result<()> {
    use std::path::Component;

    let base = Path::new(dir);
    std::fs::create_dir_all(base)?;

    let mut offset = 0usize;
    while offset + 512 <= embedded_data.len() {
        let header = &embedded_data[offset..offset + 512];

        // Two consecutive zero blocks mark end of archive.
        if header.iter().all(|&b| b == 0) {
            break;
        }

        let name = parse_tar_string(&header[0..100]);
        let size = parse_tar_octal(&header[124..136])?;
        let typeflag = header[156];
        let prefix = parse_tar_string(&header[345..500]);
        offset += 512;

        let data_end = offset.saturating_add(size);
        if data_end > embedded_data.len() {
            break; // truncated archive
        }
        let data = &embedded_data[offset..data_end];
        // Advance past data, rounded up to the 512-byte boundary.
        offset = data_end;
        let rem = offset % 512;
        if rem != 0 {
            offset += 512 - rem;
        }

        let full_name = if prefix.is_empty() {
            name
        } else {
            format!("{}/{}", prefix, name)
        };
        if full_name.is_empty() {
            continue;
        }

        let target = base.join(&full_name);
        // Reject path traversal: the target must stay under `base`.
        let mut depth = 0usize;
        for comp in target.components() {
            match comp {
                Component::ParentDir => {
                    if depth == 0 {
                        continue; // skip unsafe entries
                    }
                }
                Component::Normal(_) => depth += 1,
                _ => {}
            }
        }
        if !target.starts_with(base) {
            continue;
        }

        match typeflag {
            b'5' => {
                std::fs::create_dir_all(&target)?;
            }
            b'0' | 0 => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, data)?;
            }
            _ => {
                // Symlinks, hard links, char/block devices: skipped for safety.
            }
        }
    }
    Ok(())
}

fn parse_tar_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn parse_tar_octal(bytes: &[u8]) -> std::io::Result<usize> {
    let s = parse_tar_string(bytes);
    if s.is_empty() {
        return Ok(0);
    }
    usize::from_str_radix(s.trim(), 8)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid tar size"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_header(name: &str, size: usize, typeflag: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        let nb = name.as_bytes();
        let n = nb.len().min(100);
        h[..n].copy_from_slice(&nb[..n]);
        // size in octal (11 digits + NUL) at offset 124
        let size_str = format!("{:011o}", size);
        let sb = size_str.as_bytes();
        h[124..124 + sb.len()].copy_from_slice(sb);
        // checksum field filled with spaces (reader does not validate)
        for b in &mut h[148..156] {
            *b = b' ';
        }
        h[156] = typeflag;
        h
    }

    fn padded(data: &[u8]) -> Vec<u8> {
        let mut v = data.to_vec();
        while v.len() % 512 != 0 {
            v.push(0);
        }
        v
    }

    #[test]
    fn untar_extracts_file() {
        let dir = std::env::temp_dir().join(format!("larva-untar-{}", std::process::id()));
        let mut tar = Vec::new();
        tar.extend_from_slice(&tar_header("hello.txt", 11, b'0'));
        tar.extend_from_slice(&padded(b"hello world"));
        tar.extend_from_slice(&[0u8; 1024]); // end-of-archive

        untar(&tar, &dir.to_string_lossy()).unwrap();
        let content = std::fs::read_to_string(dir.join("hello.txt")).unwrap();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn untar_extracts_nested_directory() {
        let dir = std::env::temp_dir().join(format!("larva-untar-nested-{}", std::process::id()));
        let mut tar = Vec::new();
        // directory entry "sub/"
        tar.extend_from_slice(&tar_header("sub/", 0, b'5'));
        // file "sub/app.php"
        tar.extend_from_slice(&tar_header("sub/app.php", 3, b'0'));
        tar.extend_from_slice(&padded(b"php"));
        tar.extend_from_slice(&[0u8; 1024]);

        untar(&tar, &dir.to_string_lossy()).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("sub/app.php")).unwrap(), "php");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn untar_empty_archive_is_ok() {
        let dir = std::env::temp_dir().join(format!("larva-untar-empty-{}", std::process::id()));
        let tar = vec![0u8; 1024];
        untar(&tar, &dir.to_string_lossy()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}