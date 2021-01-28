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
}
