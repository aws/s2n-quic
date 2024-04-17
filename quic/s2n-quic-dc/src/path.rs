use s2n_quic_core::{
    path::{Handle, MaxMtu, Tuple},
    varint::VarInt,
};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub trait Controller {
    type Handle: Handle;

    fn handle(&self) -> &Self::Handle;
}

impl Controller for Tuple {
    type Handle = Self;

    #[inline]
    fn handle(&self) -> &Self::Handle {
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub max_mtu: MaxMtu,
    pub remote_max_data: VarInt,
    pub local_max_data: VarInt,
}

impl Default for Parameters {
    fn default() -> Self {
        static DEFAULT_MAX_DATA: once_cell::sync::Lazy<VarInt> = once_cell::sync::Lazy::new(|| {
            std::env::var("DC_QUIC_DEFAULT_MAX_DATA")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1u32 << 25)
                .into()
        });

        static DEFAULT_MTU: once_cell::sync::Lazy<MaxMtu> = once_cell::sync::Lazy::new(|| {
            let mtu = if cfg!(target_os = "linux") {
                8940
            } else {
                1450
            };

            std::env::var("DC_QUIC_DEFAULT_MTU")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(mtu)
                .try_into()
                .unwrap()
        });

        Self {
            max_mtu: *DEFAULT_MTU,
            remote_max_data: *DEFAULT_MAX_DATA,
            local_max_data: *DEFAULT_MAX_DATA,
        }
    }
}
