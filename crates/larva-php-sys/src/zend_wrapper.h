#ifndef LARVA_PHP_ZEND_WRAPPER_H
#define LARVA_PHP_ZEND_WRAPPER_H

/*
 * Minimal Zend Engine header wrapper for bindgen.
 *
 * When PHP is installed on the system, PHP_INCLUDES should point to the
 * PHP source/include directory so bindgen can parse the full Zend headers.
 * Otherwise, only the opaque forward declarations are generated and the
 * FFI functions must be declared manually in src/ffi.rs.
 */

#ifdef HAVE_PHP_INCLUDES
#include <php.h>
#include <zend.h>
#include <zend_API.h>
#include <zend_alloc.h>
#include <zend_hash.h>
#include <zend_llist.h>
#include <zend_modules.h>
#include <zend_types.h>
#include <zend_string.h>
#include <zend_enum.h>
#include <SAPI.h>
#include <php_variables.h>
#include <php_ini.h>
#include <php_main.h>
#include <php_output.h>
#include <php_version.h>
#endif

/* Opaque types always available for FFI */
typedef struct _zval_struct zval;
typedef struct _zend_string zend_string;
typedef struct _zend_array zend_array;
typedef struct _zend_module_entry zend_module_entry;
typedef struct _sapi_module_struct sapi_module_struct;
typedef struct _sapi_headers_struct sapi_headers_struct;
typedef struct _sapi_request_info sapi_request_info;
typedef struct _zend_llist zend_llist;
typedef struct _zend_file_handle zend_file_handle;
typedef struct _zend_class_entry zend_class_entry;
typedef struct _zend_object zend_object;
typedef struct _zend_reference zend_reference;
typedef struct _Bucket Bucket;

/* Constants always available */
#define SUCCESS 0
#define FAILURE (-1)

/* zval type constants (from Zend/zend_types.h) */
#define IS_UNDEF 0
#define IS_NULL 1
#define IS_FALSE 2
#define IS_TRUE 3
#define IS_LONG 4
#define IS_DOUBLE 5
#define IS_STRING 6
#define IS_ARRAY 7
#define IS_OBJECT 8
#define IS_RESOURCE 9
#define IS_REFERENCE 10
#define IS_PTR 13

#endif /* LARVA_PHP_ZEND_WRAPPER_H */