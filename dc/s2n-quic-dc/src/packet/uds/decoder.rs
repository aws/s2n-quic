// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::uds::encoder::{APP_PARAMS_VERSION, PACKET_VERSION, PACKET_VERSION_V1},
    path::secret::schedule::Ciphersuite,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, DecoderError};
use s2n_quic_core::{dc::ApplicationParams, varint::VarInt};

#[derive(Clone, Debug)]
pub struct Packet {
    version_tag: u8,
    ciphersuite: Ciphersuite,
    export_secret: Vec<u8>,
    application_params_version: u8,
    application_params: ApplicationParams,
    encode_time: u64, // CLOCK_MONOTONIC_RAW in microseconds
    application_data: Option<Vec<u8>>,
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
    pub fn encode_time(&self) -> u64 {
        self.encode_time
    }

    #[inline]
    pub fn application_data(&self) -> Option<&[u8]> {
        self.application_data.as_deref()
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[inline(always)]
    pub fn decode(buffer: DecoderBufferMut) -> DecoderBufferMutResult<Packet> {
        let (version_tag, buffer) = buffer.decode::<u8>()?;

        if version_tag != PACKET_VERSION && version_tag != PACKET_VERSION_V1 {
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

        let (encode_time, buffer) = buffer.decode::<u64>()?;

        let (application_data, buffer) = if version_tag == PACKET_VERSION_V1 {
            let (application_data_slice, buffer) =
                buffer.decode_slice_with_len_prefix::<VarInt>()?;
            let application_data_slice = application_data_slice.into_less_safe_slice();
            let application_data = if application_data_slice.is_empty() {
                None
            } else {
                Some(application_data_slice.to_vec())
            };
            (application_data, buffer)
        } else {
            (None, buffer)
        };

        let (payload_slice, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;
        let payload = payload_slice.into_less_safe_slice().to_vec();

        let packet = Packet {
            version_tag,
            ciphersuite,
            export_secret,
            application_params_version,
            application_params,
            encode_time,
            application_data,
            payload,
        };

        Ok((packet, buffer))
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        packet::uds::{
            decoder,
            encoder::{self, PACKET_VERSION, PACKET_VERSION_V1},
        },
        path::secret::schedule::Ciphersuite,
    };
    use s2n_codec::{DecoderBufferMut, DecoderError, Encoder, EncoderLenEstimator};
    use s2n_quic_core::dc;

    /// Encodes a packet (estimating the buffer size first) and returns the encoded bytes.
    fn encode_packet(
        ciphersuite: &Ciphersuite,
        export_secret: &[u8],
        application_params: &dc::ApplicationParams,
        encode_time: u64,
        application_data: Option<&[u8]>,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut estimator = EncoderLenEstimator::new(usize::MAX);
        let expected_size = encoder::encode(
            &mut estimator,
            ciphersuite,
            export_secret,
            application_params,
            encode_time,
            application_data,
            payload,
        );
        let mut buffer = vec![0u8; expected_size];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut buffer);
        let encoded_size = encoder::encode(
            &mut enc,
            ciphersuite,
            export_secret,
            application_params,
            encode_time,
            application_data,
            payload,
        );
        assert_eq!(encoded_size, expected_size);
        buffer
    }

    fn assert_application_params_eq(
        decoded_params: &dc::ApplicationParams,
        application_params: &dc::ApplicationParams,
    ) {
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
    fn test_encode_decode() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";
        let time = 0;

        let mut buffer = encode_packet(
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            None,
            payload,
        );

        // Decode
        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        // Verify
        assert_eq!(packet.version_tag(), PACKET_VERSION);
        assert_eq!(packet.ciphersuite(), ciphersuite);
        assert_eq!(packet.export_secret(), export_secret);
        assert_eq!(packet.encode_time(), time);
        assert_eq!(packet.application_data(), None);
        assert_eq!(packet.payload(), payload);

        assert_application_params_eq(packet.application_params(), &application_params);
    }

    /// A v1 packet with a non-empty application-data blob round-trips and
    /// exposes the blob unmodified via the accessor.
    #[test]
    fn test_encode_decode_v1_with_application_data() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";
        let application_data = b"opaque_application_data_blob";
        let time = 123;

        let mut buffer = encode_packet(
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            Some(application_data),
            payload,
        );

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION_V1);
        assert_eq!(packet.ciphersuite(), ciphersuite);
        assert_eq!(packet.export_secret(), export_secret);
        assert_eq!(packet.encode_time(), time);
        assert_eq!(packet.application_data(), Some(&application_data[..]));
        assert_eq!(packet.payload(), payload);

        assert_application_params_eq(packet.application_params(), &application_params);
    }

    /// Encoding with `application_data: None` always produces a v0 packet,
    /// which decodes with `application_data() == None`.
    #[test]
    fn test_encode_none_application_data_produces_v0() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";
        let time = 0;

        let mut buffer = encode_packet(
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            None,
            payload,
        );

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION);
        assert_eq!(packet.application_data(), None);
    }

    /// A hand-built v0 buffer (matching the pre-existing wire format, no
    /// application-data field) still decodes correctly.
    #[test]
    fn test_decode_hand_built_v0_buffer() {
        let mut buffer = vec![
            0u8, // version tag = 0
            0u8, // ciphersuite = 0 (valid)
            4u8, // export secret length = 4
            b't', b'e', b's', b't', // export secret
            0u8,  // application params version = 0
        ];

        // application params
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let mut params_buf = vec![0u8; 128];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut params_buf);
        enc.encode(&application_params);
        let params_len = enc.len();
        buffer.extend_from_slice(&params_buf[..params_len]);

        // encode_time
        buffer.extend_from_slice(&0u64.to_be_bytes());

        // payload, length-prefixed (varint length = 4)
        buffer.push(4u8);
        buffer.extend_from_slice(b"data");

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION);
        assert_eq!(packet.export_secret(), b"test");
        assert_eq!(packet.application_data(), None);
        assert_eq!(packet.payload(), b"data");
    }

    /// A zero-length application-data blob in a v1 packet decodes to `None`,
    /// since an empty serialization is defined as "nothing to forward".
    #[test]
    fn test_decode_zero_length_application_data_maps_to_none() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let payload = b"payload_with_data";
        let time = 0;

        let mut buffer = encode_packet(
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            Some(&[][..]),
            payload,
        );

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        // A zero-length blob still bumps the version tag to v1 on encode...
        assert_eq!(packet.version_tag(), PACKET_VERSION_V1);
        // ...but the accessor maps it to `None`.
        assert_eq!(packet.application_data(), None);
        assert_eq!(packet.payload(), payload);
    }

    #[test]
    fn test_decode_invalid_version_tag() {
        let mut buffer = vec![2u8, 0u8]; // Unknown version tag = 2 (neither 0 nor 1)
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
