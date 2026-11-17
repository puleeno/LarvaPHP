//! C bridge for LarvaPHP - wraps PHP SAPI embed functions from php8.dll.
//!
//! Links against php8.lib (import library for php8.dll).
//! All Zend types are opaque pointers - we don't include PHP headers.

#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <string.h>

// Opaque PHP types (match ffi.rs declarations)
typedef struct zval zval;
typedef struct zend_string zend_string;
typedef struct zend_array zend_array;
typedef struct zend_object zend_object;
typedef struct zend_class_entry zend_class_entry;
typedef struct zend_module_entry zend_module_entry;
typedef struct sapi_module_struct sapi_module_struct;

// ---- PHP SAPI embed functions (from php8.lib / php8.dll) ----

// These are declared extern and resolved at link time against php8.lib
extern int php_module_startup(void *sapi_module, void *additional_modules, int num_additional_modules);
extern int php_module_shutdown(void);
extern void php_request_startup(void);
extern void php_request_shutdown(void);
extern int php_execute_script(void *file_handle);
extern void php_register_variable_safe(const char *key, const char *val, size_t val_len, zval *track_vars_array);
extern void php_register_variable(const char *key, const char *val, size_t val_len, zval *track_vars_array);
extern void *php_output_get_contents(void);
extern void php_output_activate(void);
extern void php_output_end_all(void);
extern int zend_stream_init_filename(void *handle, const char *filename);
extern int zend_stream_open(void *handle);
extern void zend_destroy_file_handle(void *handle);

// ---- Output capture helper ----

/// Execute a PHP script and capture its output.
/// Returns a malloc'd null-terminated string (caller must free), or NULL on error.
char *larva_php_execute_and_capture(const char *script_path) {
    php_output_activate();
    
    // Open and execute the script file
    void *file_handle = malloc(sizeof(void *) * 4); // rough size for zend_file_handle
    if (!file_handle) return NULL;
    memset(file_handle, 0, sizeof(void *) * 4);
    
    if (zend_stream_init_filename(file_handle, script_path) != 0) {
        free(file_handle);
        return NULL;
    }
    
    php_execute_script(file_handle);
    zend_destroy_file_handle(file_handle);
    free(file_handle);
    
    // Get output buffer contents
    void *output = php_output_get_contents();
    php_output_end_all();
    
    if (!output) return NULL;
    
    // Convert zend_string to C string (zend_string has len + val[1])
    // Layout: [zend_refcounted_h gc][ulong h][size_t len][char val[1]]
    // We skip the header to get to len and val
    unsigned char *p = (unsigned char *)output;
    // Skip gc (8 bytes: refcount 4 + u 4)
    p += 8;
    // Skip h (8 bytes)
    p += 8;
    // Read len (8 bytes on 64-bit)
    size_t len = *(size_t *)p;
    p += 8;
    // val starts here
    char *val = (char *)p;
    
    // Allocate and copy
    char *result = malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, val, len);
    result[len] = '\0';
    
    return result;
}

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
    frankenphp_version_t v = {8, 3, 33, "", "8.3.33", 80333};
    return v;
}

frankenphp_config_t frankenphp_get_config(void) {
    frankenphp_config_t c = {0, 0, 0};
    return c;
}

// ---- Thread management ----

static int g_main_thread_started = 0;
static unsigned long g_thread_index = 0;

int frankenphp_new_main_thread(int num_threads) {
    (void)num_threads;
    if (g_main_thread_started) return 0;
    // PHP module startup will be called from Rust side via ffi
    g_main_thread_started = 1;
    return 0;
}

int frankenphp_new_php_thread(unsigned long thread_index) {
    g_thread_index = thread_index;
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
    return g_thread_index;
}

// ---- Context management ----

void frankenphp_update_local_thread_context(int is_worker) {
    (void)is_worker;
}

int frankenphp_shutdown_dummy_request(void) {
    php_request_shutdown();
    return 1;
}

int frankenphp_execute_script_cli(char *script, int argc, char **argv, int eval) {
    (void)script;
    (void)argc;
    (void)argv;
    (void)eval;
    // TODO: implement CLI script execution via php_execute_script
    return 0;
}

// ---- Environment variables ----

void frankenphp_register_known_variable(void *z_key, char *value, unsigned long val_len, void *track_vars_array) {
    (void)z_key;
    (void)value;
    (void)val_len;
    (void)track_vars_array;
}

void frankenphp_register_variable_safe(char *key, char *val, unsigned long val_len, void *track_vars_array) {
    php_register_variable_safe(key, val, val_len, (zval *)track_vars_array);
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

// ---- Force kill ----

typedef int force_kill_slot;

void frankenphp_force_kill_thread(force_kill_slot slot) {
    (void)slot;
}

void frankenphp_release_thread_for_kill(force_kill_slot slot) {
    (void)slot;
}

// ---- Extension registration ----

void register_extensions(zend_module_entry **m, int len) {
    (void)m;
    (void)len;
}

// ---- Interned strings ----

typedef struct {
    void *strings;
} frankenphp_interned_strings_t;

frankenphp_interned_strings_t frankenphp_strings = {0};

// ---- Zend hash string update ----

zval *__hash_update_string__(zend_array *ht, zend_string *k, zend_string *v) {
    (void)ht;
    (void)k;
    (void)v;
    return NULL;
}

void larva_php_free_string(char *s) {
    free(s);
}
