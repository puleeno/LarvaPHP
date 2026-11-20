//! PHP INI resolver for the embedded runtime (Strategy + Chain of Responsibility).
//!
//! The embedded PHP (`php8.dll`) does not automatically find `php.ini` when
//! loaded from a non-PHP executable. This module resolves the php.ini path and
//! builds fallback INI entries (for `extension_dir` / `extension=`) so
//! extensions like `mysqli` load reliably.
//!
//! Design:
//! - [`PhpIniResolver`] trait (Strategy) — each resolver knows how to find config.
//! - [`EnvResolver`] — uses the `PHPRC` environment variable.
//! - [`PathResolver`] — explicit candidate paths (e.g. the PHP install dir).
//! - [`IniEntriesResolver`] — builds `key=value` INI entries as a fallback that
//!   does not depend on a php.ini file existing.
//! - [`ChainResolver`] — tries each resolver in order (Chain of Responsibility).

use std::path::PathBuf;

/// The resolved PHP configuration for the embedded runtime.
#[derive(Debug, Clone, Default)]
pub struct PhpIniConfig {
    /// Path to `php.ini` (passed via `php_ini_path_override`).
    pub path: Option<PathBuf>,
    /// Directory containing `php.ini` (passed via `PHPRC`).
    pub dir: Option<PathBuf>,
    /// Fallback INI entries (`key=value\n...`), applied via `ini_entries`.
    pub entries: Option<String>,
}

/// Strategy: resolve PHP INI configuration.
pub trait PhpIniResolver: Send + Sync {
    fn name(&self) -> &str;
    fn resolve(&self) -> PhpIniConfig;
}

/// Resolves via the `PHPRC` environment variable (directory containing php.ini).
pub struct EnvResolver;

impl PhpIniResolver for EnvResolver {
    fn name(&self) -> &str {
        "env"
    }
    fn resolve(&self) -> PhpIniConfig {
        if let Ok(dir) = std::env::var("PHPRC") {
            let dir = PathBuf::from(dir);
            let ini = dir.join("php.ini");
            if ini.exists() {
                return PhpIniConfig {
                    path: Some(ini),
                    dir: Some(dir),
                    entries: None,
                };
            }
        }
        PhpIniConfig::default()
    }
}

/// Resolves via a list of explicit candidate php.ini paths.
pub struct PathResolver {
    candidates: Vec<PathBuf>,
}

impl PathResolver {
    pub fn new(candidates: Vec<PathBuf>) -> Self {
        Self { candidates }
    }
}

impl PhpIniResolver for PathResolver {
    fn name(&self) -> &str {
        "path"
    }
    fn resolve(&self) -> PhpIniConfig {
        for c in &self.candidates {
            if c.exists() {
                let dir = c.parent().map(|p| p.to_path_buf());
                return PhpIniConfig {
                    path: Some(c.clone()),
                    dir,
                    entries: None,
                };
            }
        }
        PhpIniConfig::default()
    }
}

/// Builds fallback INI entries so extensions load even without a php.ini file.
///
/// This reads the `extension_dir` from a discovered php.ini (if any) and emits
/// `extension=<name>` entries for the requested extensions.
pub struct IniEntriesResolver {
    /// Extensions to enable (e.g. `["mysqli", "pdo_mysql"]`).
    extensions: Vec<String>,
}

impl IniEntriesResolver {
    pub fn new(extensions: Vec<String>) -> Self {
        Self { extensions }
    }
}

impl PhpIniResolver for IniEntriesResolver {
    fn name(&self) -> &str {
        "entries"
    }
    fn resolve(&self) -> PhpIniConfig {
        let mut entries = String::new();
        // Discover extension_dir from a php.ini, if present.
        if let Some(ini) = find_php_ini() {
            if let Ok(content) = std::fs::read_to_string(&ini) {
                for line in content.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("extension_dir") {
                        if let Some(val) = rest.split('=').nth(1) {
                            let val = val.trim().trim_matches('"');
                            if !val.is_empty() {
                                entries.push_str(&format!("extension_dir=\"{}\"\n", val));
                            }
                            break;
                        }
                    }
                }
            }
        }
        for ext in &self.extensions {
            entries.push_str(&format!("extension={}\n", ext));
        }
        PhpIniConfig {
            path: None,
            dir: None,
            entries: Some(entries),
        }
    }
}

/// Chain of Responsibility: try each resolver, merge results.
pub struct ChainResolver {
    resolvers: Vec<Box<dyn PhpIniResolver>>,
}

impl ChainResolver {
    pub fn new(resolvers: Vec<Box<dyn PhpIniResolver>>) -> Self {
        Self { resolvers }
    }
}

impl PhpIniResolver for ChainResolver {
    fn name(&self) -> &str {
        "chain"
    }
    fn resolve(&self) -> PhpIniConfig {
        let mut merged = PhpIniConfig::default();
        for r in &self.resolvers {
            let c = r.resolve();
            if merged.path.is_none() {
                merged.path = c.path;
            }
            if merged.dir.is_none() {
                merged.dir = c.dir;
            }
            if merged.entries.is_none() {
                merged.entries = c.entries;
            }
        }
        merged
    }
}

/// Locate `php.ini` from common Windows locations.
fn find_php_ini() -> Option<PathBuf> {
    for c in [
        r"C:\Program Files\PHP\current\php.ini",
        r"C:\Program Files\PHP\8.3.33\nts\x64\php.ini",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Build the default resolver chain for the embedded runtime.
pub fn default_resolver() -> impl PhpIniResolver {
    ChainResolver::new(vec![
        Box::new(EnvResolver),
        Box::new(PathResolver::new(vec![
            PathBuf::from(r"C:\Program Files\PHP\current\php.ini"),
            PathBuf::from(r"C:\Program Files\PHP\8.3.33\nts\x64\php.ini"),
        ])),
        Box::new(IniEntriesResolver::new(vec![
            "mysqli".to_string(),
            "pdo_mysql".to_string(),
        ])),
    ])
}

/// Resolve the PHP INI configuration using the default resolver chain.
pub fn resolve() -> PhpIniConfig {
    default_resolver().resolve()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolver_returns_config() {
        let cfg = resolve();
        // On a system with PHP, path or entries should be populated.
        // The entries resolver always returns Some (even if empty-ish).
        assert!(cfg.path.is_some() || cfg.entries.is_some());
    }

    #[test]
    fn entries_resolver_includes_extensions() {
        let r = IniEntriesResolver::new(vec!["mysqli".to_string()]);
        let cfg = r.resolve();
        let entries = cfg.entries.unwrap();
        assert!(entries.contains("extension=mysqli"));
    }

    #[test]
    fn chain_resolver_merges() {
        let r = ChainResolver::new(vec![
            Box::new(PathResolver::new(vec![PathBuf::from("/nonexistent/php.ini")])),
            Box::new(IniEntriesResolver::new(vec!["mysqli".to_string()])),
        ]);
        let cfg = r.resolve();
        // Path not found -> entries populated.
        assert!(cfg.path.is_none());
        assert!(cfg.entries.is_some());
    }
}