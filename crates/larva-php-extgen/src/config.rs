//! Extension generator configuration.
//!
//! Mirrors the configuration options from `internal/extgen/`.

use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for the extension generator.
#[derive(Debug, Clone, Default)]
pub struct ExtGenConfig {
    /// Directory containing PHP extension source files.
    pub source_dir: PathBuf,
    /// Output file for generated Rust bindings.
    pub output_file: PathBuf,
    /// Include directories for bindgen.
    pub include_dirs: Vec<PathBuf>,
    /// Define macros.
    pub defines: HashMap<String, String>,
    /// Clang args.
    pub clang_args: Vec<String>,
    /// Allowlist for types.
    pub allowlist_types: Vec<String>,
    /// Allowlist for functions.
    pub allowlist_functions: Vec<String>,
    /// Allowlist for variables.
    pub allowlist_vars: Vec<String>,
    /// Blocklist for types.
    pub blocklist_types: Vec<String>,
    /// Blocklist for functions.
    pub blocklist_functions: Vec<String>,
    /// Blocklist for variables.
    pub blocklist_vars: Vec<String>,
}

impl ExtGenConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Extension metadata.
#[derive(Debug, Clone, Default)]
pub struct ExtensionMeta {
    /// Extension name.
    pub name: String,
    /// Extension version.
    pub version: String,
    /// Required PHP version.
    pub php_version: Option<String>,
    /// Functions provided by this extension.
    pub functions: Vec<FunctionMeta>,
    /// Classes provided by this extension.
    pub classes: Vec<ClassMeta>,
    /// Constants provided by this extension.
    pub constants: Vec<ConstantMeta>,
}

/// Function metadata.
#[derive(Debug, Clone, Default)]
pub struct FunctionMeta {
    pub name: String,
    pub parameters: Vec<ParameterMeta>,
    pub return_type: String,
    pub docs: Option<String>,
}

/// Parameter metadata.
#[derive(Debug, Clone, Default)]
pub struct ParameterMeta {
    pub name: String,
    pub type_name: String,
    pub default_value: Option<String>,
    pub nullable: bool,
    pub variadic: bool,
}

/// Class metadata.
#[derive(Debug, Clone, Default)]
pub struct ClassMeta {
    pub name: String,
    pub parent: Option<String>,
    pub interfaces: Vec<String>,
    pub properties: Vec<PropertyMeta>,
    pub methods: Vec<MethodMeta>,
}

/// Property metadata.
#[derive(Debug, Clone, Default)]
pub struct PropertyMeta {
    pub name: String,
    pub type_name: String,
    pub default_value: Option<String>,
    pub public: bool,
    pub protected: bool,
    pub private: bool,
    pub static_property: bool,
}

/// Method metadata.
#[derive(Debug, Clone, Default)]
pub struct MethodMeta {
    pub name: String,
    pub parameters: Vec<ParameterMeta>,
    pub return_type: String,
    pub public: bool,
    pub protected: bool,
    pub private: bool,
    pub static_method: bool,
    pub abstract_method: bool,
}

/// Constant metadata.
#[derive(Debug, Clone, Default)]
pub struct ConstantMeta {
    pub name: String,
    pub value: String,
    pub type_name: String,
}