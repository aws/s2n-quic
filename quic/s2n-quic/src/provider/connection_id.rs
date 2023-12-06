// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides connection id support for an endpoint

pub use s2n_quic_core::connection::id::{ConnectionInfo, Format, Generator, LocalId, Validator};

pub trait Provider: 'static {
    type Format: 'static + Format;
    type Error: core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Format, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

impl<T: 'static + Format> Provider for T {
    type Format = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Format, Self::Error> {
        Ok(self)
    }
}

pub mod default {
    use core::{
        convert::{Infallible, TryInto},
        time::Duration,
    };
    use rand::prelude::*;
    use s2n_quic_core::connection::{
        self,
        id::{ConnectionInfo, Generator, Validator},
    };

    #[derive(Debug, Default)]
    pub struct Provider(Format);

    impl super::Provider for Provider {
        type Format = Format;
        type Error = Infallible;

        fn start(self) -> Result<Self::Format, Self::Error> {
            Ok(self.0)
        }
    }

    /// 16 bytes should be big enough for a randomly generated Id
    const DEFAULT_LEN: usize = 16;

    /// Randomly generated connection Id format.
    ///
    /// By default, connection Ids of length 16 bytes are generated.
    #[derive(Debug)]
    pub struct Format {
        len: usize,
        lifetime: Option<Duration>,
        rotate_handshake_connection_id: bool,
    }

    impl Default for Format {
        fn default() -> Self {
            Self {
                len: DEFAULT_LEN,
                lifetime: None,
                rotate_handshake_connection_id: true,
            }
        }
    }

    impl Format {
        /// Creates a builder for the format
        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    /// A builder for [`Format`] providers
    #[derive(Debug)]
    pub struct Builder {
        len: usize,
        lifetime: Option<Duration>,
        rotate_handshake_connection_id: bool,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self {
                len: DEFAULT_LEN,
                lifetime: None,
                rotate_handshake_connection_id: true,
            }
        }
    }

    impl Builder {
        /// Sets the length of the generated connection Id
        pub fn with_len(mut self, len: usize) -> Result<Self, connection::id::Error> {
            if !(connection::LocalId::MIN_LEN..=connection::id::MAX_LEN).contains(&len) {
                return Err(connection::id::Error::InvalidLength);
            }
            self.len = len;
            Ok(self)
        }

        /// Sets the lifetime of each generated connection Id
        pub fn with_lifetime(mut self, lifetime: Duration) -> Result<Self, connection::id::Error> {
            if !(connection::id::MIN_LIFETIME..=connection::id::MAX_LIFETIME).contains(&lifetime) {
                return Err(connection::id::Error::InvalidLifetime);
            }
            self.lifetime = Some(lifetime);
            Ok(self)
        }

        /// Enables/disables rotation of the connection Id used during the handshake (default: enabled)
        ///
        /// When enabled (the default), the connection ID used during the the handshake
        /// will be requested to be retired following confirmation of the handshake
        /// completing. This reduces linkability between information exchanged
        /// during and after the handshake.
        pub fn with_handshake_connection_id_rotation(
            mut self,
            enabled: bool,
        ) -> Result<Self, core::convert::Infallible> {
            self.rotate_handshake_connection_id = enabled;
            Ok(self)
        }

        /// Builds the [`Format`] into a provider
        pub fn build(self) -> Result<Format, core::convert::Infallible> {
            Ok(Format {
                len: self.len,
                lifetime: self.lifetime,
                rotate_handshake_connection_id: self.rotate_handshake_connection_id,
            })
        }
    }

    impl Generator for Format {
        fn generate(&mut self, _connection_info: &ConnectionInfo) -> connection::LocalId {
            let mut id = [0u8; connection::id::MAX_LEN];
            let id = &mut id[..self.len];
            rand::thread_rng().fill_bytes(id);
            (&*id).try_into().expect("length already checked")
        }

        fn lifetime(&self) -> Option<Duration> {
            self.lifetime
        }

        fn rotate_handshake_connection_id(&self) -> bool {
            self.rotate_handshake_connection_id
        }
    }

    impl Validator for Format {
        fn validate(&self, _connection_info: &ConnectionInfo, buffer: &[u8]) -> Option<usize> {
            if buffer.len() >= self.len {
                Some(self.len)
            } else {
                None
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn generator_test() {
            let remote_address = &s2n_quic_core::inet::SocketAddress::default();
            let connection_info = ConnectionInfo::new(remote_address);

            for len in connection::LocalId::MIN_LEN..connection::id::MAX_LEN {
                let mut format = Format::builder().with_len(len).unwrap().build().unwrap();

                let id = format.generate(&connection_info);

                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.2
                //= type=test
                //# An endpoint that uses this design MUST
                //# either use the same connection ID length for all connections or
                //# encode the length of the connection ID such that it can be recovered
                //# without state.
                assert_eq!(format.validate(&connection_info, id.as_ref()), Some(len));
                assert_eq!(id.len(), len);
                assert_eq!(format.lifetime(), None);
                assert!(format.rotate_handshake_connection_id());
            }

            assert_eq!(
                Some(connection::id::Error::InvalidLength),
                Format::builder()
                    .with_len(connection::id::MAX_LEN + 1)
                    .err()
            );

            assert_eq!(
                Some(connection::id::Error::InvalidLength),
                Format::builder()
                    .with_len(connection::LocalId::MIN_LEN - 1)
                    .err()
            );

            let lifetime = Duration::from_secs(1000);
            let format = Format::builder()
                .with_lifetime(lifetime)
                .unwrap()
                .build()
                .unwrap();
            assert_eq!(Some(lifetime), format.lifetime());
            assert!(format.rotate_handshake_connection_id());

            assert_eq!(
                Some(connection::id::Error::InvalidLifetime),
                Format::builder()
                    .with_lifetime(connection::id::MIN_LIFETIME - Duration::from_millis(1))
                    .err()
            );

            assert_eq!(
                Some(connection::id::Error::InvalidLifetime),
                Format::builder()
                    .with_lifetime(connection::id::MAX_LIFETIME + Duration::from_millis(1))
                    .err()
            );

            let format = Format::builder().build().unwrap();
            assert!(format.rotate_handshake_connection_id());

            let format = Format::builder()
                .with_handshake_connection_id_rotation(true)
                .unwrap()
                .build()
                .unwrap();
            assert!(format.rotate_handshake_connection_id());

            let format = Format::builder()
                .with_handshake_connection_id_rotation(false)
                .unwrap()
                .build()
                .unwrap();
            assert!(!format.rotate_handshake_connection_id());
        }
    }
}
