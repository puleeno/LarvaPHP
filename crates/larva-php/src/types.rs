//! Safe wrappers for zval conversion (PHP ↔ Rust).
//!
//! Mirrors `types.go` from the Go source. Provides type-safe conversion between
//! Rust values and PHP zvals.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use crate::ffi;
use crate::Result;
use crate::errors::Error;

/// HASH_FLAG_PACKED constant (from Zend/zend_types.h)
const HASH_FLAG_PACKED: u32 = 0x00000004;

/// A PHP value that can be converted to/from a zval.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpValue {
    Null,
    Bool(bool),
    Long(i64),
    Double(f64),
    String(String),
    Array(Vec<PhpValue>),
    AssociativeArray(AssociativeArray),
}

/// A PHP associative array with ordered key-value pairs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssociativeArray {
    pub map: HashMap<String, PhpValue>,
    pub order: Vec<String>,
}

impl AssociativeArray {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: PhpValue) {
        let key = key.into();
        if !self.map.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.map.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&PhpValue> {
        self.map.get(key)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Convert a Zend string pointer to a Rust String.
///
/// # Safety
///
/// `s` must be a valid pointer to a `zend_string`.
pub unsafe fn go_string(s: *const ffi::zend_string) -> String {
    if s.is_null() {
        return String::new();
    }
    let zs = &*s;
    let val_ptr = &zs.val as *const c_char;
    let bytes = std::slice::from_raw_parts(val_ptr as *const u8, zs.len);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Convert a Rust string to a Zend string (non-persistent, request memory).
///
/// Returns null for empty strings.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn php_string(s: &str, persistent: bool) -> *mut ffi::zend_string {
    if s.is_empty() {
        return ptr::null_mut();
    }
    let cstr = CString::new(s).unwrap_or_default();
    ffi::zend_string_init(cstr.as_ptr(), s.len(), persistent)
}

/// Check if a zend_array is packed (list-like).
///
/// # Safety
///
/// `arr` must be a valid pointer to a `zend_array`.
pub unsafe fn ht_is_packed(ht: *const ffi::zend_array) -> bool {
    if ht.is_null() {
        return false;
    }
    let flags = (*ht).u.flags;
    (flags & HASH_FLAG_PACKED) != 0
}

/// Convert a zend_array to a Rust Vec<PhpValue> (packed array).
///
/// # Safety
///
/// `arr` must be a valid pointer to a `zend_array`.
pub unsafe fn go_packed_array(arr: *const ffi::zend_array) -> Result<Vec<PhpValue>> {
    if arr.is_null() {
        return Err(Error::Ffi("null array pointer".into()));
    }

    let ht = &*arr;
    protect_recursion(arr as *mut _)?;

    let n = ht.nNumUsed;
    let mut result = Vec::with_capacity(n as usize);

    if ht_is_packed(arr) {
        for i in 0..n {
            let v = ffi::get_ht_packed_data(arr as *mut _, i);
            if !v.is_null() && ffi::zval_get_type(v) != ffi::zval_type::UNDEF {
                result.push(go_value(v)?);
            }
        }
    } else {
        for i in 0..n {
            let bucket = ffi::get_ht_bucket_data(arr as *mut _, i);
            if !bucket.is_null() && ffi::zval_get_type(&(*bucket).val) != ffi::zval_type::UNDEF {
                result.push(go_value(&(*bucket).val)?);
            }
        }
    }

    ffi::__zend_array_unprotect_recursion__(arr as *mut _);
    Ok(result)
}

/// Convert a zend_array to an AssociativeArray.
///
/// # Safety
///
/// `arr` must be a valid pointer to a `zend_array`.
pub unsafe fn go_associative_array(arr: *const ffi::zend_array) -> Result<AssociativeArray> {
    if arr.is_null() {
        return Err(Error::Ffi("null array pointer".into()));
    }

    let ht = &*arr;
    protect_recursion(arr as *mut _)?;

    let n = ht.nNumUsed;
    let mut result = AssociativeArray::new();

    if ht_is_packed(arr) {
        for i in 0..n {
            let v = ffi::get_ht_packed_data(arr as *mut _, i);
            if !v.is_null() && ffi::zval_get_type(v) != ffi::zval_type::UNDEF {
                let key = i.to_string();
                result.insert(key, go_value(v)?);
            }
        }
    } else {
        for i in 0..n {
            let bucket = ffi::get_ht_bucket_data(arr as *mut _, i);
            if bucket.is_null() || ffi::zval_get_type(&(*bucket).val) == ffi::zval_type::UNDEF {
                continue;
            }
            let key = if !(*bucket).key.is_null() {
                go_string((*bucket).key)
            } else {
                (*bucket).h.to_string()
            };
            result.insert(key, go_value(&(*bucket).val)?);
        }
    }

    ffi::__zend_array_unprotect_recursion__(arr as *mut _);
    Ok(result)
}

/// Convert a zval to a PhpValue.
///
/// # Safety
///
/// `zval` must be a valid pointer to a `zval`.
pub unsafe fn go_value(zv: *const ffi::zval) -> Result<PhpValue> {
    if zv.is_null() {
        return Ok(PhpValue::Null);
    }

    let mut zv = zv;
    let mut t = ffi::zval_get_type(zv);

    if t == ffi::zval_type::REFERENCE {
        zv = *(*zv).value.as_ptr() as *mut ffi::zval;
        t = ffi::zval_get_type(zv);
    }

    match t {
        ffi::zval_type::NULL => Ok(PhpValue::Null),
        ffi::zval_type::FALSE => Ok(PhpValue::Bool(false)),
        ffi::zval_type::TRUE => Ok(PhpValue::Bool(true)),
        ffi::zval_type::LONG => {
            let val = *(*zv).value.as_ptr() as i64;
            Ok(PhpValue::Long(val))
        }
        ffi::zval_type::DOUBLE => {
            let val = *(*zv).value.as_ptr() as f64;
            Ok(PhpValue::Double(val))
        }
        ffi::zval_type::STRING => {
            let zs = *(*zv).value.as_ptr() as *mut ffi::zend_string;
            if zs.is_null() {
                Ok(PhpValue::String(String::new()))
            } else {
                Ok(PhpValue::String(go_string(zs)))
            }
        }
        ffi::zval_type::ARRAY => {
            let arr = *(*zv).value.as_ptr() as *mut ffi::zend_array;
            if !arr.is_null() && ht_is_packed(arr) {
                Ok(PhpValue::Array(go_packed_array(arr)?))
            } else {
                Ok(PhpValue::AssociativeArray(go_associative_array(arr)?))
            }
        }
        _ => Err(Error::Ffi(format!("unsupported zval type {}", t))),
    }
}

/// Convert a PhpValue to a zval.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn php_value(value: &PhpValue) -> *mut ffi::zval {
    let mut zv: ffi::zval = std::mem::zeroed();
    php_value_into(value, &mut zv);

    let zval = ffi::__emalloc__(std::mem::size_of::<ffi::zval>()) as *mut ffi::zval;
    *zval = zv;
    zval
}

/// Convert a PhpValue into an existing zval.
///
/// # Safety
///
/// `zval` must be a valid pointer to a `zval`.
pub unsafe fn php_value_into(value: &PhpValue, zval: *mut ffi::zval) {
    match value {
        PhpValue::Null => ffi::__zval_null__(zval),
        PhpValue::Bool(b) => ffi::__zval_bool__(zval, *b),
        PhpValue::Long(l) => ffi::__zval_long__(zval, *l),
        PhpValue::Double(d) => ffi::__zval_double__(zval, *d),
        PhpValue::String(s) => {
            if s.is_empty() {
                ffi::__zval_empty_string__(zval);
            } else {
                let zs = php_string(s, false);
                ffi::__zval_string__(zval, zs);
            }
        }
        PhpValue::Array(arr) => {
            let zend_arr = php_packed_array(arr);
            ffi::__zval_arr__(zval, zend_arr);
        }
        PhpValue::AssociativeArray(aa) => {
            let zend_arr = php_associative_array(aa);
            ffi::__zval_arr__(zval, zend_arr);
        }
    }
}

/// Create a new zend_array with the specified size.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn create_new_array(size: u32) -> *mut ffi::zend_array {
    ffi::__zend_new_array__(size)
}

/// Convert a Rust Vec to a PHP packed array.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn php_packed_array(slice: &[PhpValue]) -> *mut ffi::zend_array {
    let zend_arr = create_new_array(slice.len() as u32);
    for val in slice {
        let zval = php_value(val);
        ffi::zend_hash_next_index_insert(zend_arr, zval);
        ffi::__efree__(zval as *mut _);
    }
    zend_arr
}

/// Convert an AssociativeArray to a PHP associative array.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn php_associative_array(aa: &AssociativeArray) -> *mut ffi::zend_array {
    let zend_arr = create_new_array(aa.len() as u32);
    for key in &aa.order {
        let val = aa.map.get(key).unwrap();
        let zval = php_value(val);
        let ckey = CString::new(key.as_str()).unwrap_or_default();
        ffi::zend_hash_str_update(
            zend_arr,
            ckey.as_ptr(),
            key.len() as usize,
            zval,
        );
        ffi::__efree__(zval as *mut _);
    }
    zend_arr
}

/// Protect a hash table from recursive conversion.
unsafe fn protect_recursion(ht: *mut ffi::zend_array) -> Result<()> {
    if ffi::__zend_array_is_recursive__(ht) {
        return Err(Error::Ffi("cannot convert a recursive array".into()));
    }
    ffi::__zend_array_protect_recursion__(ht);
    Ok(())
}

/// Release a zend_string.
///
/// # Safety
///
/// `p` must be a valid pointer to a `zend_string`.
pub unsafe fn zend_string_release(p: *mut ffi::zend_string) {
    if !p.is_null() {
        ffi::zend_string_release(p);
    }
}

/// Destroy a zend_array (hash table).
///
/// # Safety
///
/// `p` must be a valid pointer to a `zend_array`.
pub unsafe fn zend_hash_destroy(p: *mut ffi::zend_array) {
    if !p.is_null() {
        ffi::zend_hash_destroy(p);
    }
}

/// Call a PHP callable with the given parameters.
///
/// Returns the result as a PhpValue, or Null if the call failed.
///
/// # Safety
///
/// `cb` must be a valid pointer to a callable zval.
/// The Zend Engine must be initialized.
pub unsafe fn call_php_callable(cb: *const ffi::zval, params: &[PhpValue]) -> PhpValue {
    if cb.is_null() {
        return PhpValue::Null;
    }

    if ffi::__zend_is_callable__(cb as *mut _) == 0 {
        return PhpValue::Null;
    }

    let param_count = params.len();
    let param_storage = if param_count > 0 {
        let storage = ffi::__emalloc__(
            param_count * std::mem::size_of::<ffi::zval>()
        ) as *mut ffi::zval;
        for (i, param) in params.iter().enumerate() {
            let target = storage.add(i);
            let source = php_value(param);
            std::ptr::write(target, std::ptr::read(source));
            ffi::__efree__(source as *mut _);
        }
        storage
    } else {
        ptr::null_mut()
    };

    let mut retval: ffi::zval = std::mem::zeroed();
    let result = ffi::__call_user_function__(
        cb as *mut _,
        &mut retval,
        param_count as u32,
        param_storage as *mut _,
    );

    if !param_storage.is_null() {
        for i in 0..param_count {
            let target = param_storage.add(i);
            ffi::zval_ptr_dtor(target);
        }
        ffi::__efree__(param_storage as *mut _);
    }

    if result != 0 {
        return PhpValue::Null;
    }

    let go_result = go_value(&retval).unwrap_or(PhpValue::Null);
    ffi::zval_ptr_dtor(&mut retval);
    go_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_value_enum_variants() {
        assert_eq!(PhpValue::Null, PhpValue::Null);
        assert_eq!(PhpValue::Bool(true), PhpValue::Bool(true));
        assert_eq!(PhpValue::Bool(false), PhpValue::Bool(false));
        assert_eq!(PhpValue::Long(42), PhpValue::Long(42));
        assert_eq!(PhpValue::Double(3.14), PhpValue::Double(3.14));
        assert_eq!(PhpValue::String("hello".to_string()), PhpValue::String("hello".to_string()));
    }

    #[test]
    fn associative_array_insert_and_get() {
        let mut aa = AssociativeArray::new();
        assert!(aa.is_empty());
        assert_eq!(aa.len(), 0);

        aa.insert("foo", PhpValue::Long(1));
        aa.insert("bar", PhpValue::String("baz".to_string()));
        assert_eq!(aa.len(), 2);
        assert!(!aa.is_empty());

        assert_eq!(aa.get("foo"), Some(&PhpValue::Long(1)));
        assert_eq!(aa.get("bar"), Some(&PhpValue::String("baz".to_string())));
        assert_eq!(aa.get("missing"), None);
    }

    #[test]
    fn associative_array_insert_duplicate_updates_order() {
        let mut aa = AssociativeArray::new();
        aa.insert("a", PhpValue::Long(1));
        aa.insert("b", PhpValue::Long(2));
        aa.insert("a", PhpValue::Long(3));
        assert_eq!(aa.len(), 2);
        assert_eq!(aa.order, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(aa.get("a"), Some(&PhpValue::Long(3)));
    }

    #[test]
    fn associative_array_default() {
        let aa: AssociativeArray = Default::default();
        assert!(aa.is_empty());
    }

    #[test]
    fn php_value_debug() {
        let v = PhpValue::Long(42);
        assert!(format!("{:?}", v).contains("42"));
        let v = PhpValue::String("test".to_string());
        assert!(format!("{:?}", v).contains("test"));
    }
}