// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::CStr, sync::Arc};

/// Trait which a user must implement to verify host name(s) during X509 verification when using
/// mutual TLS.
pub trait VerifyHostContext: Send + Sync {
    fn verify_certificate_host_name(&self, host_name: &str) -> u8;
}

/// Holds a reference an implementation that will verify the certificate host name.
pub struct VerifyHostContextWrapper {
    pub handle: Box<dyn VerifyHostContext>,
}

impl VerifyHostContextWrapper {
    pub fn new(verify_host_ctx: Box<dyn VerifyHostContext>) -> VerifyHostContextHandle {
        Arc::new(VerifyHostContextWrapper {
            handle: verify_host_ctx,
        })
    }

    /// # Safety
    /// The verify_host_context is passed to the callback as a raw mutable pointer. No thread
    /// safety is maintained on this pointer and it is the responsibility of the callback passed in
    /// to enforce mutual exclusion on the memory pointed to by the context handle.
    pub unsafe extern "C" fn verify_host_callback(
        host_name: *const ::libc::c_char,
        _host_name_len: usize,
        data: *mut ::libc::c_void,
    ) -> u8 {
        let maybe_cstr = CStr::from_ptr(host_name).to_str();
        if let Ok(host_name_str) = maybe_cstr {
            let ctx = &mut *(data as *mut VerifyHostContextWrapper);
            ctx.handle.verify_certificate_host_name(host_name_str)
        } else {
            // If the host name can't be parsed, fail closed.
            0
        }
    }
}

pub type VerifyHostContextHandle = Arc<VerifyHostContextWrapper>;
