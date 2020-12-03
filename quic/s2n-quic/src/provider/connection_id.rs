use s2n_quic_core::connection::id::Format;
pub use s2n_quic_core::connection::id::{Generator, Validator};

/// Provides connection id support for an endpoint
pub trait Provider: 'static {
    type Format: 'static + Format;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::Format, Self::Error>;
}

cfg_if::cfg_if! {
    if #[cfg(feature = "rand")] {
        pub use random::Provider as Default;
    } else {
        // TODO implement stub that panics
    }
}

impl_provider_utils!();

#[cfg(feature = "rand")]
pub mod random {
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

    impl super::TryInto for Format {
        type Provider = Provider;
        type Error = Infallible;

        fn try_into(self) -> Result<Self::Provider, Self::Error> {
            Ok(Provider(self))
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
    }

    impl Default for Format {
        fn default() -> Self {
            Self {
                len: DEFAULT_LEN,
                lifetime: None,
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
    }

    impl Default for Builder {
        fn default() -> Self {
            Self {
                len: DEFAULT_LEN,
                lifetime: None,
            }
        }
    }

    impl Builder {
        /// Sets the length of the generated connection Id
        pub fn with_len(mut self, len: usize) -> Result<Self, connection::id::Error> {
            if len > connection::id::MAX_LEN {
                return Err(connection::id::Error::InvalidLength);
            }
            self.len = len;
            Ok(self)
        }

        /// Sets the lifetime of each generated connection Id
        pub fn with_lifetime(mut self, lifetime: Duration) -> Result<Self, connection::id::Error> {
            if lifetime < connection::id::MIN_LIFETIME {
                return Err(connection::id::Error::InvalidLifetime);
            }
            self.lifetime = Some(lifetime);
            Ok(self)
        }

        /// Builds the [`Format`] into a provider
        pub fn build(self) -> Result<Format, core::convert::Infallible> {
            Ok(Format {
                len: self.len,
                lifetime: self.lifetime,
            })
        }
    }

    impl Generator for Format {
        fn generate(&mut self, _connection_info: &ConnectionInfo) -> connection::Id {
            let mut id = [0u8; connection::id::MAX_LEN];
            let id = &mut id[..self.len];
            rand::thread_rng().fill_bytes(id);
            (&id[..]).try_into().expect("length already checked")
        }

        fn lifetime(&self) -> Option<Duration> {
            self.lifetime
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

    #[test]
    fn generator_test() {
        let remote_address = &s2n_quic_core::inet::SocketAddress::default();
        let connection_info = ConnectionInfo::new(remote_address);

        for len in 0..connection::id::MAX_LEN {
            let mut format = Format::builder().with_len(len).unwrap().build().unwrap();

            let id = format.generate(&connection_info);
            assert_eq!(format.validate(&connection_info, id.as_ref()), Some(len));
            assert_eq!(id.len(), len);
            assert_eq!(format.lifetime(), None);
        }

        assert_eq!(
            Some(connection::id::Error::InvalidLength),
            Format::builder()
                .with_len(connection::id::MAX_LEN + 1)
                .err()
        );

        let lifetime = Duration::from_secs(1000);
        let format = Format::builder()
            .with_lifetime(lifetime)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(Some(lifetime), format.lifetime());

        assert_eq!(
            Some(connection::id::Error::InvalidLifetime),
            Format::builder()
                .with_lifetime(connection::id::MIN_LIFETIME - Duration::from_millis(1))
                .err()
        );
    }
}
