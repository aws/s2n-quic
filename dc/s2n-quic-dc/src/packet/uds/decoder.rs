// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::uds::encoder::{APP_PARAMS_VERSION, PACKET_VERSION},
    path::secret::schedule::Ciphersuite,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, DecoderError};
use s2n_quic_core::{dc::ApplicationParams, time::Timestamp, varint::VarInt};

#[derive(Clone, Debug)]
pub struct Packet {
    version_tag: u8,
    ciphersuite: Ciphersuite,
    export_secret: Vec<u8>,
    application_params_version: u8,
    application_params: ApplicationParams,
    queue_time: Timestamp,
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
    pub fn queue_time(&self) -> &Timestamp {
        &self.queue_time
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[inline(always)]
    pub fn decode(buffer: DecoderBufferMut) -> DecoderBufferMutResult<Packet> {
        let (version_tag, buffer) = buffer.decode::<u8>()?;

        if version_tag != PACKET_VERSION {
            return Err(DecoderError::InvariantViolation("Unsupported version tag"));
        }

        let (ciphersuite_byte, buffer) = buffer.decode::<u8>()?;
        let ciphersuite = ciphersuite_byte
            .try_into()
            .map_err(DecoderError::InvariantViolation)?;

        let (export_secret_slice, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;
        let export_secret = export_secret_slice.into_less_safe_slice().to_vec();

        let (application_params_version, buffer) = buffer.decode::<u8>()?;

        if application_params_version != APP_PARAMS_VERSION {
            return Err(DecoderError::InvariantViolation(
                "Unsupported application parameters version",
            ));
        }

        let (application_params, buffer) = buffer.decode::<ApplicationParams>()?;

        let (queue_time, buffer) = buffer.decode::<Timestamp>()?;

        let (payload_slice, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;
        let payload = payload_slice.into_less_safe_slice().to_vec();

        let packet = Packet {
            version_tag,
            ciphersuite,
            export_secret,
            application_params_version,
            application_params,
            queue_time,
            payload,
        };

        Ok((packet, buffer))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        packet::uds::{
            decoder,
            encoder::{self, PACKET_VERSION},
        },
        path::secret::schedule::Ciphersuite,
    };
    use s2n_codec::{DecoderBufferMut, DecoderError, EncoderLenEstimator};
    use s2n_quic_core::{dc, time::Timestamp};

    #[test]
    fn test_encode_decode() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";
        let time = unsafe { Timestamp::from_duration(Duration::new(10, 0)) };

        // Encode
        let mut estimator = EncoderLenEstimator::new(usize::MAX);
        let expected_size = encoder::encode(
            &mut estimator,
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            payload,
        );
        let mut buffer = vec![0u8; expected_size];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut buffer);
        let encoded_size = encoder::encode(
            &mut enc,
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            payload,
        );
        assert_eq!(encoded_size, expected_size);

        // Decode
        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        let decoded_params = packet.application_params();

        // Verify
        assert_eq!(packet.version_tag(), PACKET_VERSION);
        assert_eq!(packet.ciphersuite(), ciphersuite);
        assert_eq!(packet.export_secret(), export_secret);
        assert_eq!(packet.payload(), payload);

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
                assert_eq!(msg, "Unsupported version tag");
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
                assert_eq!(msg, "Unsupported application parameters version");
            }
            _ => panic!("Expected InvariantViolation error"),
        }
    }
}
