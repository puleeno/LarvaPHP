//! Minimal C stubs for FrankenPHP-specific functions.
//!
//! These provide linker symbols for the FFI declarations in ffi.rs.
//! No PHP headers needed - all PHP types are opaque pointers.

#include <stddef.h>

// Opaque PHP types (match ffi.rs declarations)
typedef struct zval zval;
typedef struct zend_string zend_string;
typedef struct zend_array zend_array;
typedef struct zend_object zend_object;
typedef struct zend_class_entry zend_class_entry;
typedef struct zend_module_entry zend_module_entry;
typedef struct sapi_module_struct sapi_module_struct;

// ---- Version and config ----

typedef struct {
    unsigned char major_version;
    unsigned char minor_version;
    unsigned char release_version;
    const char *extra_version;
    const char *version;
    unsigned long version_id;
} frankenphp_version_t;

typedef struct {
    int zts;
    int zend_signals;
    int zend_max_execution_timers;
} frankenphp_config_t;

frankenphp_version_t frankenphp_get_version(void) {
    frankenphp_version_t v = {8, 3, 33, "", "8.3.33 (stub)", 80333};
    return v;
}

frankenphp_config_t frankenphp_get_config(void) {
    frankenphp_config_t c = {0, 0, 0};
    return c;
}

// ---- Thread management (stubs) ----

int frankenphp_new_main_thread(int num_threads) {
    (void)num_threads;
    return 0;
}

int frankenphp_new_php_thread(unsigned long thread_index) {
    (void)thread_index;
    return 1;
}

void frankenphp_init_thread_metrics(int max_threads) {
    (void)max_threads;
}

void frankenphp_destroy_thread_metrics(void) {
}

unsigned long frankenphp_get_thread_memory_usage(unsigned long thread_index) {
    (void)thread_index;
    return 0;
}

unsigned long frankenphp_thread_index(void) {
    return 0;
}

// ---- Context management (stubs) ----

void frankenphp_update_local_thread_context(int is_worker) {
    (void)is_worker;
}

int frankenphp_shutdown_dummy_request(void) {
    return 0;
}

int frankenphp_execute_script_cli(char *script, int argc, char **argv, int eval) {
    (void)script;
    (void)argc;
    (void)argv;
    (void)eval;
    return 0;
}

// ---- Environment variables (stubs) ----

void frankenphp_register_known_variable(void *z_key, char *value, unsigned long val_len, void *track_vars_array) {
    (void)z_key;
    (void)value;
    (void)val_len;
    (void)track_vars_array;
}

void frankenphp_register_variable_safe(char *key, char *val, unsigned long val_len, void *track_vars_array) {
    (void)key;
    (void)val;
    (void)val_len;
    (void)track_vars_array;
}

void frankenphp_register_server_vars(void *track_vars_array, void *vars) {
    (void)track_vars_array;
    (void)vars;
}

void frankenphp_add_to_prepared_env(char *name, unsigned long name_len, char *val, unsigned long val_len, unsigned long size) {
    (void)name;
    (void)name_len;
    (void)val;
    (void)val_len;
    (void)size;
}

void *frankenphp_init_persistent_string(const char *string, unsigned long len) {
    (void)string;
    (void)len;
    return NULL;
}

int frankenphp_get_current_memory_limit(void) {
    return -1;
}

// ---- Force kill (stubs) ----

typedef int force_kill_slot;

void frankenphp_force_kill_thread(force_kill_slot slot) {
    (void)slot;
}

void frankenphp_release_thread_for_kill(force_kill_slot slot) {
    (void)slot;
}

// ---- Extension registration (stub) ----

void register_extensions(zend_module_entry **m, int len) {
    (void)m;
    (void)len;
}

// ---- Interned strings (stub) ----

typedef struct {
    void *strings;
} frankenphp_interned_strings_t;

frankenphp_interned_strings_t frankenphp_strings = {0};

// ---- Zend hash string update (stub for symbol that's referenced but not in php8.lib) ----

zval *__hash_update_string__(zend_array *ht, zend_string *k, zend_string *v) {
    (void)ht;
    (void)k;
    (void)v;
    return NULL;
}
