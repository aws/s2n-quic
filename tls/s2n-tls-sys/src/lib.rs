#![no_std]
#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code
)]

pub use libc::c_int as s2n_status_code;

#[cfg(vendored)]
include!("./vendored.rs");

#[cfg(not(vendored))]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

impl From<s2n_status_code> for s2n_error_type {
    fn from(code: s2n_status_code) -> Self {
        // It's not safe to transmute an int into an enum so we need to match each value
        //
        // We have a test below to catch any consistencies
        // See: https://github.com/awslabs/s2n/blob/44d7dce7c4a5a16cfd13cc85bee591790d516f60/api/s2n.h#L63-L72
        match code {
            0 => s2n_error_type::Ok,
            1 => s2n_error_type::Io,
            2 => s2n_error_type::Closed,
            3 => s2n_error_type::Blocked,
            4 => s2n_error_type::Alert,
            5 => s2n_error_type::Proto,
            6 => s2n_error_type::Internal,
            7 => s2n_error_type::Usage,
            // if the type doesn't match just return an internal error
            _ => s2n_error_type::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_connection() {
        unsafe {
            let connection = s2n_connection_new(s2n_mode::Server);
            let config = s2n_config_new();
            s2n_config_enable_quic(config);
            s2n_connection_set_config(connection, config);
        }
    }

    #[test]
    fn error_kind_test() {
        use s2n_error_type::*;
        let types = [Ok, Io, Closed, Blocked, Alert, Proto, Internal, Usage];

        // make sure the conversion is correct
        for ty in types.iter().copied() {
            assert_eq!(ty, (ty as s2n_status_code).into());
        }

        // make sure s2n only returns types 0..=7
        for code in 0..1024 {
            let kind = unsafe { s2n_error_get_type(code) };
            assert!((0..=7).contains(&kind));
        }
    }
}
