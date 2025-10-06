// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::uds::encoder::{APP_PARAMS_VERSION, PACKET_VERSION},
    path::secret::schedule::Ciphersuite,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, DecoderError};
use s2n_quic_core::{
    connection::Limits,
    dc::ApplicationParams,
    transport::parameters::{InitialFlowControlLimits, InitialStreamLimits},
    varint::VarInt,
};
use std::{num::NonZeroU32, time::Duration};

#[derive(Clone, Debug)]
pub struct Packet {
    version_tag: u8,
    ciphersuite: Ciphersuite,
    export_secret: Vec<u8>,
    application_params_version: u8,
    application_params: ApplicationParams,
    payload: Vec<u8>,
}

impl Packet {
    #[inline]
    pub fn version_tag(&self) -> u8 {
        self.version_tag
    }

    #[inline]
    pub fn ciphersuite(&self) -> Ciphersuite {
        self.ciphersuite
    }

    #[inline]
    pub fn export_secret(&self) -> &[u8] {
        &self.export_secret
    }

    #[inline]
    pub fn application_params_version(&self) -> u8 {
        self.application_params_version
    }

    #[inline]
    pub fn application_params(&self) -> &ApplicationParams {
        &self.application_params
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[inline]
    #[cfg(test)]
    pub fn total_len(&self) -> usize {
        use super::encoder::application_params_encoding_size;
        use s2n_codec::EncoderValue as _;
        1 + // version tag
        1 + // ciphersuite
        VarInt::try_from(self.export_secret.len()).unwrap().encoding_size() +
        self.export_secret.len() +
        1 + // application params version
        application_params_encoding_size(&self.application_params) +
        VarInt::try_from(self.payload.len()).unwrap().encoding_size() +
        self.payload.len()
    }

    pub fn decode(buffer: DecoderBufferMut) -> DecoderBufferMutResult<Packet> {
        let (version_tag, buffer) = buffer.decode::<u8>()?;

        if version_tag != PACKET_VERSION {
            return Err(DecoderError::InvariantViolation("unsupported version tag"));
        }

        let (ciphersuite_byte, buffer) = buffer.decode::<u8>()?;
        let ciphersuite = ciphersuite_byte
            .try_into()
            .map_err(DecoderError::InvariantViolation)?;

        let (export_secret_len, buffer) = buffer.decode::<VarInt>()?;
        let export_secret_len = *export_secret_len as usize;

        let (export_secret_slice, buffer) = buffer.decode_slice(export_secret_len)?;
        let export_secret = export_secret_slice.into_less_safe_slice().to_vec();

        let (application_params_version, buffer) = buffer.decode::<u8>()?;

        if application_params_version != APP_PARAMS_VERSION {
            return Err(DecoderError::InvariantViolation(
                "unsupported application parameters version",
            ));
        }

        let (application_params, buffer) = Self::decode_application_params(buffer)?;

        let (payload_len, buffer) = buffer.decode::<VarInt>()?;
        let payload_len = *payload_len as usize;

        let (payload_slice, buffer) = buffer.decode_slice(payload_len)?;
        let payload = payload_slice.into_less_safe_slice().to_vec();

        let packet = Packet {
            version_tag,
            ciphersuite,
            export_secret,
            application_params_version,
            application_params,
            payload,
        };

        Ok((packet, buffer))
    }

    fn decode_application_params(
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ApplicationParams> {
        let (max_datagram_size, buffer) = buffer.decode::<u16>()?;
        let (remote_max_data, buffer) = buffer.decode::<VarInt>()?;
        let (local_send_max_data, buffer) = buffer.decode::<VarInt>()?;
        let (local_recv_max_data, buffer) = buffer.decode::<VarInt>()?;

        let (presence_flag, buffer) = buffer.decode::<u8>()?;
        let (max_idle_timeout, buffer) = match presence_flag {
            0 => (None, buffer),
            1 => {
                let (timeout_value, buffer) = buffer.decode::<u32>()?;
                let timeout = NonZeroU32::new(timeout_value).ok_or(
                    DecoderError::InvariantViolation("max_idle_timeout cannot be zero"),
                )?;
                (Some(timeout), buffer)
            }
            _ => {
                return Err(DecoderError::InvariantViolation(
                    "invalid max_idle_timeout presence flag",
                ))
            }
        };

        let peer_flow_control_limits = InitialFlowControlLimits {
            stream_limits: InitialStreamLimits {
                // unused in ApplicationParams::new()
                max_data_bidi_local: VarInt::from_u32(0),
                max_data_bidi_remote: VarInt::from_u32(0),
                max_data_uni: VarInt::from_u32(0),
            },
            max_data: remote_max_data,
            max_open_remote_bidirectional_streams: VarInt::from_u32(0),
            max_open_remote_unidirectional_streams: VarInt::from_u32(0),
        };

        let mut limits = Limits::new()
            .with_bidirectional_local_data_window(local_send_max_data.as_u64())
            .map_err(|_| DecoderError::InvariantViolation("invalid local_send_max_data"))?
            .with_bidirectional_remote_data_window(local_recv_max_data.as_u64())
            .map_err(|_| DecoderError::InvariantViolation("invalid local_recv_max_data"))?;
        if let Some(timeout) = max_idle_timeout {
            let timeout_duration = Duration::from_millis(timeout.get() as u64);
            limits = limits
                .with_max_idle_timeout(timeout_duration)
                .map_err(|_| DecoderError::InvariantViolation("invalid max_idle_timeout"))?;
        }

        let application_params =
            ApplicationParams::new(max_datagram_size, &peer_flow_control_limits, &limits);

        Ok((application_params, buffer))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        packet::uds::{
            decoder,
            encoder::{self, PACKET_VERSION},
        },
        path::secret::schedule::Ciphersuite,
    };
    use s2n_codec::{DecoderBufferMut, DecoderError};
    use s2n_quic_core::dc;
    #[test]
    fn test_encode_decode() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data_123";
        let export_secret_len = export_secret.len().try_into().unwrap();
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";

        // Encode
        let expected_size = encoder::encoding_size(
            export_secret,
            export_secret_len,
            &application_params,
            payload,
        );
        let mut buffer = vec![0u8; expected_size];
        let enc = s2n_codec::EncoderBuffer::new(&mut buffer);
        let encoded_size = encoder::encode(
            enc,
            &ciphersuite,
            export_secret,
            export_secret_len,
            &application_params,
            payload,
        );
        assert_eq!(encoded_size, expected_size);

        // Decode
        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        let decoded_params = packet.application_params();

        // Verify
        assert_eq!(packet.version_tag, PACKET_VERSION);
        assert_eq!(packet.ciphersuite(), ciphersuite);
        assert_eq!(packet.export_secret(), export_secret);
        assert_eq!(packet.payload(), payload);
        assert_eq!(packet.total_len(), expected_size);

        use core::sync::atomic::Ordering;
        assert_eq!(
            decoded_params.max_datagram_size.load(Ordering::Relaxed),
            application_params.max_datagram_size.load(Ordering::Relaxed)
        );
        assert_eq!(
            decoded_params.remote_max_data,
            application_params.remote_max_data
        );
        assert_eq!(
            decoded_params.local_send_max_data,
            application_params.local_send_max_data
        );
        assert_eq!(
            decoded_params.local_recv_max_data,
            application_params.local_recv_max_data
        );
        assert_eq!(
            decoded_params.max_idle_timeout,
            application_params.max_idle_timeout
        );
    }

    #[test]
    fn test_decode_invalid_version_tag() {
        let mut buffer = vec![1u8, 0u8]; // Invalid version tag = 1
        let decoder = DecoderBufferMut::new(&mut buffer);
        let result = decoder::Packet::decode(decoder);
        assert!(result.is_err());
        match result.unwrap_err() {
            DecoderError::InvariantViolation(msg) => {
                assert_eq!(msg, "unsupported version tag");
            }
            _ => panic!("Expected InvariantViolation error"),
        }
    }

    #[test]
    fn test_decode_invalid_app_params_version() {
        let mut buffer = vec![
            0u8, // version tag = 0
            0u8, // ciphersuite = 0 (valid)
            4u8, // export secret length = 4
            b't', b'e', b's', b't', // export secret
            1u8,  // invalid application params version = 1
        ];
        let decoder = DecoderBufferMut::new(&mut buffer);
        let result = decoder::Packet::decode(decoder);
        assert!(result.is_err());
        match result.unwrap_err() {
            DecoderError::InvariantViolation(msg) => {
                assert_eq!(msg, "unsupported application parameters version");
            }
            _ => panic!("Expected InvariantViolation error"),
        }
    }
}
