// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection, inet::SocketAddress, random};

#[non_exhaustive]
pub struct Context<'a> {
    pub peer_address: &'a SocketAddress,
    pub destination_connection_id: &'a connection::PeerId,
    pub random: &'a mut dyn random::Generator,
}

impl<'a> Context<'a> {
    pub fn new(
        peer_address: &'a SocketAddress,
        destination_connection_id: &'a connection::PeerId,
        random: &'a mut dyn random::Generator,
    ) -> Self {
        Self {
            peer_address,
            destination_connection_id,
            random,
        }
    }
}

pub trait Format: 'static {
    const TOKEN_LEN: usize;

    /// Generate a signed token to be delivered in a NEW_TOKEN frame.
    /// This function will only be called if the provider support NEW_TOKEN frames.
    fn generate_new_token(
        &mut self,
        context: &mut Context<'_>,
        source_connection_id: &connection::LocalId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Generate a signed token to be delivered in a Retry Packet
    fn generate_retry_token(
        &mut self,
        context: &mut Context<'_>,
        original_destination_connection_id: &connection::InitialId,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Return the original destination connection id of a valid token.
    /// If the token is invalid, return None.
    /// Callers should detect duplicate tokens and treat them as invalid.
    fn validate_token(
        &mut self,
        context: &mut Context<'_>,
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

    #[derive(Debug, Default)]
    pub struct Format(u64);

    impl super::Format for Format {
        const TOKEN_LEN: usize = retry::example::TOKEN_LEN;

        fn generate_new_token(
            &mut self,
            _context: &mut Context<'_>,
            _source_connection_id: &connection::LocalId,
            _output_buffer: &mut [u8],
        ) -> Option<()> {
            // TODO implement one for testing
            None
        }

        fn generate_retry_token(
            &mut self,
            _context: &mut Context<'_>,
            _original_destination_connection_id: &connection::InitialId,
            output_buffer: &mut [u8],
        ) -> Option<()> {
            output_buffer.copy_from_slice(&retry::example::TOKEN);
            Some(())
        }

        fn validate_token(
            &mut self,
            _context: &mut Context<'_>,
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
