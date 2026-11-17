//! Mercure real-time capabilities.
//!
//! Mirrors `mercure.go` and `mercure-skip.go` from the Go source.

#[cfg(feature = "mercure")]
pub mod imp {
    use crate::errors::Result;
    use crate::ffi;

    /// A Mercure update.
    #[derive(Debug, Clone)]
    pub struct Update {
        pub topics: Vec<String>,
        pub data: String,
        pub private: bool,
        pub id: Option<String>,
        pub event_type: Option<String>,
        pub retry: u64,
    }

    /// A Mercure hub.
    pub struct Hub {
        // TODO: implement hub
    }

    impl Hub {
        pub fn new() -> Self {
            Hub {}
        }

        pub fn publish(&self, _update: &Update) -> Result<String> {
            // TODO: implement publish
            Ok(String::new())
        }
    }

    /// Publish a Mercure update from PHP.
    ///
    /// # Safety
    ///
    /// `topics` and other zval pointers must be valid.
    pub unsafe fn go_mercure_publish(
        _thread_index: usize,
        _topics: *mut ffi::zval,
        _data: *mut ffi::zend_string,
        _private: bool,
        _id: *mut ffi::zend_string,
        _typ: *mut ffi::zend_string,
        _retry: u64,
    ) -> (*mut ffi::zend_string, *mut std::os::raw::c_char, ffi::frankenphp_mercure_status, u32) {
        (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ffi::frankenphp_mercure_status::NO_HUB,
            0,
        )
    }
}

#[cfg(feature = "mercure")]
pub use imp::*;

#[cfg(not(feature = "mercure"))]
pub mod imp {
    use crate::ffi;

    pub struct Update;
    pub struct Hub;

    impl Hub {
        pub fn new() -> Self {
            Hub
        }
    }

    /// # Safety
    /// Pointers must be valid.
    pub unsafe fn go_mercure_publish(
        _thread_index: usize,
        _topics: *mut ffi::zval,
        _data: *mut ffi::zend_string,
        _private: bool,
        _id: *mut ffi::zend_string,
        _typ: *mut ffi::zend_string,
        _retry: u64,
    ) -> (*mut ffi::zend_string, *mut std::os::raw::c_char, ffi::frankenphp_mercure_status, u32) {
        (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ffi::frankenphp_mercure_status::UNSUPPORTED,
            0,
        )
    }
}

#[cfg(not(feature = "mercure"))]
pub use imp::*;