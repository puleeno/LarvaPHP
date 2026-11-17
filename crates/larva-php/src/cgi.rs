//! CGI variable registration and path splitting.
//!
//! Mirrors `cgi.go` and `cgimethods.go` from the Go source.

use std::collections::HashMap;
use std::path::Path;

use crate::errors::{Error, Result};
use crate::ffi;

/// Fast absolute path resolution.
pub mod fastabs {
    use std::path::{Path, PathBuf};

    pub fn fast_abs(path: &str) -> std::io::Result<PathBuf> {
        let p = Path::new(path);
        if p.is_absolute() {
            Ok(p.to_path_buf())
        } else {
            std::env::current_dir().map(|cwd| cwd.join(p))
        }
    }
}

/// Common HTTP headers mapped to PHP $_SERVER variable names.
pub fn common_request_headers() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("Accept", "HTTP_ACCEPT");
    m.insert("Accept-Charset", "HTTP_ACCEPT_CHARSET");
    m.insert("Accept-Encoding", "HTTP_ACCEPT_ENCODING");
    m.insert("Accept-Language", "HTTP_ACCEPT_LANGUAGE");
    m.insert("Authorization", "HTTP_AUTHORIZATION");
    m.insert("Cache-Control", "HTTP_CACHE_CONTROL");
    m.insert("Connection", "HTTP_CONNECTION");
    m.insert("Content-Disposition", "HTTP_CONTENT_DISPOSITION");
    m.insert("Content-Encoding", "HTTP_CONTENT_ENCODING");
    m.insert("Content-Length", "HTTP_CONTENT_LENGTH");
    m.insert("Content-Type", "HTTP_CONTENT_TYPE");
    m.insert("Cookie", "HTTP_COOKIE");
    m.insert("Date", "HTTP_DATE");
    m.insert("Expect", "HTTP_EXPECT");
    m.insert("Forwarded", "HTTP_FORWARDED");
    m.insert("From", "HTTP_FROM");
    m.insert("If-Match", "HTTP_IF_MATCH");
    m.insert("If-Modified-Since", "HTTP_IF_MODIFIED_SINCE");
    m.insert("If-None-Match", "HTTP_IF_NONE_MATCH");
    m.insert("If-Range", "HTTP_IF_RANGE");
    m.insert("If-Unmodified-Since", "HTTP_IF_UNMODIFIED_SINCE");
    m.insert("Keep-Alive", "HTTP_KEEP_ALIVE");
    m.insert("Max-Forwards", "HTTP_MAX_FORWARDS");
    m.insert("Origin", "HTTP_ORIGIN");
    m.insert("Pragma", "HTTP_PRAGMA");
    m.insert("Proxy-Authorization", "HTTP_PROXY_AUTHORIZATION");
    m.insert("Range", "HTTP_RANGE");
    m.insert("Referer", "HTTP_REFERER");
    m.insert("TE", "HTTP_TE");
    m.insert("Trailer", "HTTP_TRAILER");
    m.insert("Transfer-Encoding", "HTTP_TRANSFER_ENCODING");
    m.insert("Upgrade", "HTTP_UPGRADE");
    m.insert("User-Agent", "HTTP_USER_AGENT");
    m.insert("Via", "HTTP_VIA");
    m.insert("Warning", "HTTP_WARNING");
    m.insert("X-Forwarded-For", "HTTP_X_FORWARDED_FOR");
    m.insert("X-Forwarded-Host", "HTTP_X_FORWARDED_HOST");
    m.insert("X-Forwarded-Proto", "HTTP_X_FORWARDED_PROTO");
    m.insert("X-Real-IP", "HTTP_X_REAL_IP");
    m.insert("X-Requested-With", "HTTP_X_REQUESTED_WITH");
    m
}

/// Convert a header name to a PHP variable name (HTTP_* format).
pub fn header_name(key: &str) -> String {
    let mut result = String::with_capacity(key.len() + 6);
    result.push_str("HTTP_");
    for c in key.bytes() {
        match c {
            b'a'..=b'z' => result.push((c - b'a' + b'A') as char),
            b'-' => result.push('_'),
            _ => result.push(c as char),
        }
    }
    result
}

/// Normalize split path to lowercase ASCII.
pub fn normalize_split_path(split_path: &mut [String]) -> Result<()> {
    for split in split_path.iter_mut() {
        let mut normalized = String::with_capacity(split.len());
        for c in split.bytes() {
            if c >= 0x80 {
                return Err(Error::InvalidSplitPath);
            }
            if c.is_ascii_uppercase() {
                normalized.push((c + 32) as char);
            } else {
                normalized.push(c as char);
            }
        }
        *split = normalized;
    }
    Ok(())
}

/// Split a CGI path into SCRIPT_NAME, SCRIPT_FILENAME, PATH_INFO, DOCUMENT_URI.
pub fn split_cgi_path(
    path: &str,
    split_path: &[String],
    document_root: &str,
    worker_file: Option<&str>,
    is_windows: bool,
) -> Result<(String, String, String, String)> {
    let path = clean_request_path(path, is_windows);
    let split_path = if split_path.is_empty() {
        vec![".php".to_string()]
    } else {
        split_path.to_vec()
    };

    let pos = split_pos(&path, &split_path);

    let (doc_uri, path_info) = if pos > -1 {
        let pos = pos as usize;
        (path[..pos].to_string(), path[pos..].to_string())
    } else {
        (String::new(), String::new())
    };

    if let Some(worker_file) = worker_file {
        let script_filename = worker_file.to_string();
        let script_name = if let Some(rest) = worker_file.strip_prefix(document_root) {
            if !rest.is_empty() && rest.starts_with(std::path::MAIN_SEPARATOR) {
                rest.to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        return Ok((script_filename, script_name, doc_uri, path_info));
    }

    if pos == -1 {
        return Err(Error::InvalidScriptPath);
    }

    let script_name = ensure_leading_slash(&path[..path.len() - path_info.len()]);
    let script_filename = sanitized_path_join(document_root, &script_name);

    Ok((script_filename, script_name, doc_uri, path_info))
}

/// Find the position where path should be split based on split_path.
pub fn split_pos(path: &str, split_path: &[String]) -> i32 {
    if split_path.is_empty() {
        return 0;
    }

    let path_bytes = path.as_bytes();
    let path_len = path_bytes.len();

    for split in split_path {
        let split_bytes = split.as_bytes();
        let split_len = split_bytes.len();
        if split_len == 0 || split_len > path_len {
            continue;
        }

        for i in 0..=(path_len - split_len) {
            let mut match_ = true;
            for j in 0..split_len {
                let c = path_bytes[i + j];
                if c >= 0x80 {
                    match_ = false;
                    break;
                }
                let c = if c.is_ascii_uppercase() { c + 32 } else { c };
                if c != split_bytes[j] {
                    match_ = false;
                    break;
                }
            }

            if match_ {
                let pos = i + split_len;
                if pos != path_len && path_bytes[pos] != b'/' {
                    continue;
                }
                return pos as i32;
            }
        }
    }

    -1
}

/// Clean a request path by resolving dot segments.
pub fn clean_request_path(req_path: &str, windows: bool) -> String {
    let mut req_path = if windows {
        req_path.replace('\\', "/")
    } else {
        req_path.to_string()
    };

    if !req_path.contains("/.") {
        return req_path;
    }

    if windows {
        let segments: Vec<&str> = req_path.split('/').collect();
        let cleaned: Vec<String> = segments
            .iter()
            .map(|s| {
                if *s != "." && *s != ".." {
                    s.trim_end_matches(|c: char| c == '.' || c == ' ').to_string()
                } else {
                    s.to_string()
                }
            })
            .collect();
        req_path = cleaned.join("/");
    }

    let cleaned = path_clean(&format!("/{}", req_path));
    if cleaned != "/" && req_path.ends_with('/') {
        format!("{}/", cleaned)
    } else {
        cleaned
    }
}

/// A simple path.Clean implementation.
fn path_clean(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Join paths safely against directory traversal.
pub fn sanitized_path_join(root: &str, req_path: &str) -> String {
    let root = if root.is_empty() { "." } else { root };
    let rel_path = path_clean(&ensure_leading_slash(req_path));
    let rel_path = &rel_path[1..]; // strip leading /

    if rel_path.is_empty() {
        return root.to_string();
    }

    let joined = Path::new(root).join(rel_path);
    let mut result = joined.to_string_lossy().into_owned();

    if req_path.ends_with('/') && req_path.len() > 1 {
        result.push(std::path::MAIN_SEPARATOR);
    }

    result
}

fn ensure_leading_slash(path: &str) -> String {
    if path.is_empty() || path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

/// Split a remote address into host and port.
pub fn split_remote_addr(remote_addr: &str) -> (String, String) {
    if let Some((host, port)) = remote_addr.rsplit_once(':') {
        return (host.to_string(), port.to_string());
    }
    (remote_addr.to_string(), String::new())
}

/// Get the TLS protocol name in Apache mod_ssl format.
pub fn tls_protocol(proto: u16) -> &'static str {
    match proto {
        0x0301 => "TLSv1",
        0x0302 => "TLSv1.1",
        0x0303 => "TLSv1.2",
        0x0304 => "TLSv1.3",
        _ => "",
    }
}

/// HTTP method strings (for pinCString avoidance).
pub fn c_string_http_method(method: &str) -> Option<&'static str> {
    match method {
        "GET" => Some("GET"),
        "HEAD" => Some("HEAD"),
        "POST" => Some("POST"),
        "PUT" => Some("PUT"),
        "DELETE" => Some("DELETE"),
        "CONNECT" => Some("CONNECT"),
        "OPTIONS" => Some("OPTIONS"),
        "TRACE" => Some("TRACE"),
        "PATCH" => Some("PATCH"),
        _ => None,
    }
}

/// Register CGI variables into the $_SERVER array.
///
/// # Safety
///
/// `track_vars_array` must be a valid pointer to a PHP zval array.
pub unsafe fn register_server_variables(
    remote_addr: &str,
    remote_port: &str,
    document_root: &str,
    path_info: &str,
    path_translated: &str,
    php_self: &str,
    document_uri: &str,
    script_filename: &str,
    script_name: &str,
    server_name: &str,
    server_port: &str,
    server_addr: &str,
    content_length: &str,
    server_protocol: &str,
    http_host: &str,
    request_uri: &str,
    ssl_cipher: &str,
    track_vars_array: *mut ffi::zval,
) {
    let total_vars = 29;
    let mut vars: ffi::frankenphp_server_vars = std::mem::zeroed();
    vars.total_num_vars = total_vars;

    // Set string pointers
    vars.remote_addr = remote_addr.as_ptr() as *mut _;
    vars.remote_addr_len = remote_addr.len();
    vars.remote_host = remote_addr.as_ptr() as *mut _;
    vars.remote_host_len = remote_addr.len();
    vars.remote_port = remote_port.as_ptr() as *mut _;
    vars.remote_port_len = remote_port.len();
    vars.document_root = document_root.as_ptr() as *mut _;
    vars.document_root_len = document_root.len();
    vars.path_info = path_info.as_ptr() as *mut _;
    vars.path_info_len = path_info.len();
    vars.path_translated = path_translated.as_ptr() as *mut _;
    vars.path_translated_len = path_translated.len();
    vars.php_self = php_self.as_ptr() as *mut _;
    vars.php_self_len = php_self.len();
    vars.document_uri = document_uri.as_ptr() as *mut _;
    vars.document_uri_len = document_uri.len();
    vars.script_filename = script_filename.as_ptr() as *mut _;
    vars.script_filename_len = script_filename.len();
    vars.script_name = script_name.as_ptr() as *mut _;
    vars.script_name_len = script_name.len();
    vars.server_name = server_name.as_ptr() as *mut _;
    vars.server_name_len = server_name.len();
    vars.server_port = server_port.as_ptr() as *mut _;
    vars.server_port_len = server_port.len();
    vars.server_addr = server_addr.as_ptr() as *mut _;
    vars.server_addr_len = server_addr.len();
    vars.content_length = content_length.as_ptr() as *mut _;
    vars.content_length_len = content_length.len();
    vars.server_protocol = server_protocol.as_ptr() as *mut _;
    vars.server_protocol_len = server_protocol.len();
    vars.http_host = http_host.as_ptr() as *mut _;
    vars.http_host_len = http_host.len();
    vars.request_uri = request_uri.as_ptr() as *mut _;
    vars.request_uri_len = request_uri.len();
    vars.ssl_cipher = ssl_cipher.as_ptr() as *mut _;
    vars.ssl_cipher_len = ssl_cipher.len();

    // Hard-coded values
    vars.request_scheme = ffi::frankenphp_strings.httpLowercase;
    vars.ssl_protocol = ffi::frankenphp_strings.empty;
    vars.https = ffi::frankenphp_strings.empty;

    ffi::frankenphp_register_server_vars(track_vars_array, vars);
}