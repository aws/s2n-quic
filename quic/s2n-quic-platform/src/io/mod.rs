#[macro_use]
mod macros;

#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;

#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;

#[cfg(feature = "std")]
pub mod std;

pub mod default {
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(s2n_quic_platform_socket_mmsg)] {
            pub use super::mmsg::*;
        } else if #[cfg(s2n_quic_platform_socket_msg)] {
            pub use super::msg::*;
        } else if #[cfg(feature = "std")] {
            pub use super::std::*;
        }
    }
}

// TODO everything after this is deprecated - remove after not used.

pub mod rx;
pub mod tx;

#[cfg(any(feature = "std", test))]
pub mod socket;
