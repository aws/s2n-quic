// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection, inet::SocketAddress,
    random,
};

pub trait Format: 'static {
    const TOKEN_LEN: usize;
    // type RandomGenerator: random::Generator;

    /// Generate a signed token to be delivered in a NEW_TOKEN frame.
    /// This function will only be called if the provider support NEW_TOKEN frames.
    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::PeerId,
        // rand: &dyn random::Generator,
        source_connection_id: &connection::LocalId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Generate a signed token to be delivered in a Retry Packet
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::PeerId,
        rand: &dyn random::Generator,
        original_destination_connection_id: &connection::InitialId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Return the original destination connection id of a valid token.
    /// If the token is invalid, return None.
    /// Callers should detect duplicate tokens and treat them as invalid.
    fn validate_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::PeerId,
        token: &[u8],
    ) -> Option<connection::InitialId>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    RetryPacket,
    NewTokenFrame,
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::crypto::retry;
    use crate::random;

    #[derive(Debug, Default)]
    pub struct Format(u64);

    impl super::Format for Format {
        const TOKEN_LEN: usize = retry::example::TOKEN_LEN;
        // type RandomGenerator = random::testing::Generator;

        fn generate_new_token(
            &mut self,
            _peer_address: &SocketAddress,
            _destination_connection_id: &connection::PeerId,
            // _rand: &dyn random::Generator,
            _source_connection_id: &connection::LocalId,
            _output_buffer: &mut [u8],
        ) -> Option<()> {
            // TODO implement one for testing
            None
        }

        fn generate_retry_token(
            &mut self,
            _peer_address: &SocketAddress,
            _destination_connection_id: &connection::PeerId,
            _rand: &dyn random::Generator,
            _original_destination_connection_id: &connection::InitialId,
            output_buffer: &mut [u8],
        ) -> Option<()> {
            output_buffer.copy_from_slice(&retry::example::TOKEN);
            Some(())
        }

        fn validate_token(
            &mut self,
            _peer_address: &SocketAddress,
            _destination_connection_id: &connection::PeerId,
            token: &[u8],
        ) -> Option<connection::InitialId> {
            if token == retry::example::TOKEN {
                return Some(connection::InitialId::TEST_ID);
            }

            None
        }
    }

    impl Format {
        pub fn new() -> Self {
            Self(0)
        }
    }
}
