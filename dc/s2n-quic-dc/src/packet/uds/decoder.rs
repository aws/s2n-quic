// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::uds::{
        decode_address,
        encoder::{
            APP_PARAMS_VERSION, PACKET_VERSION, PACKET_VERSION_FORWARD, PACKET_VERSION_V1,
            PACKET_VERSION_V2,
        },
        Protocol,
    },
    path::secret::schedule::Ciphersuite,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, DecoderError};
use s2n_quic_core::{dc::ApplicationParams, inet::SocketAddress, varint::VarInt};

#[derive(Clone, Debug)]
pub struct Packet {
    version_tag: u8,
    protocol: Protocol,
    ciphersuite: Ciphersuite,
    export_secret: Vec<u8>,
    application_params_version: u8,
    application_params: ApplicationParams,
    encode_time: u64, // CLOCK_MONOTONIC_RAW in microseconds
    peer_address: Option<SocketAddress>,
    local_address: Option<SocketAddress>,
    application_data: Option<Vec<u8>>,
    payload: Vec<u8>,
}

impl Packet {
    #[inline]
    pub fn version_tag(&self) -> u8 {
        self.version_tag
    }

    /// The stream's transport. Packets before v2 are always TCP.
    #[inline]
    pub fn protocol(&self) -> Protocol {
        self.protocol
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

    /// The peer's address, carried by v2 packets.
    #[inline]
    pub fn peer_address(&self) -> Option<&SocketAddress> {
        self.peer_address.as_ref()
    }

    /// The address the peer sent the initial packet to, if the sender recorded it.
    #[inline]
    pub fn local_address(&self) -> Option<&SocketAddress> {
        self.local_address.as_ref()
    }

    /// The serialized [`ApplicationData`](crate::path::secret::map::ApplicationData) blob carried
    /// by a v1 packet, if any. The bytes are opaque to the dc crate.
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

        if !matches!(
            version_tag,
            PACKET_VERSION | PACKET_VERSION_V1 | PACKET_VERSION_V2
        ) {
            return Err(DecoderError::InvariantViolation("Unsupported version tag"));
        }

        let (protocol, buffer) = if version_tag == PACKET_VERSION_V2 {
            let (protocol_byte, buffer) = buffer.decode::<u8>()?;
            let protocol =
                Protocol::try_from(protocol_byte).map_err(DecoderError::InvariantViolation)?;
            (protocol, buffer)
        } else {
            (Protocol::Tcp, buffer)
        };

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

        let (peer_address, local_address, buffer) = if version_tag == PACKET_VERSION_V2 {
            let (peer_address, buffer) = decode_address(buffer)?;
            if peer_address.is_none() {
                return Err(DecoderError::InvariantViolation("Missing peer address"));
            }
            let (local_address, buffer) = decode_address(buffer)?;
            (peer_address, local_address, buffer)
        } else {
            (None, None, buffer)
        };

        let (application_data, buffer) = if version_tag != PACKET_VERSION {
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
            protocol,
            ciphersuite,
            export_secret,
            application_params_version,
            application_params,
            encode_time,
            peer_address,
            local_address,
            application_data,
            payload,
        };

        Ok((packet, buffer))
    }
}

/// A later packet for a UDP stream that was already handed off.
#[derive(Clone, Debug)]
pub struct Forward {
    encode_time: u64, // CLOCK_MONOTONIC_RAW in microseconds
    peer_address: SocketAddress,
    payload: Vec<u8>,
}

impl Forward {
    /// Returns `true` if `packet` is encoded as a [`Forward`] rather than a [`Packet`].
    #[inline]
    pub fn is_forward(packet: &[u8]) -> bool {
        packet.first() == Some(&PACKET_VERSION_FORWARD)
    }

    #[inline]
    pub fn encode_time(&self) -> u64 {
        self.encode_time
    }

    #[inline]
    pub fn peer_address(&self) -> &SocketAddress {
        &self.peer_address
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[inline(always)]
    pub fn decode(buffer: DecoderBufferMut) -> DecoderBufferMutResult<Forward> {
        let (version_tag, buffer) = buffer.decode::<u8>()?;

        if version_tag != PACKET_VERSION_FORWARD {
            return Err(DecoderError::InvariantViolation("Unsupported version tag"));
        }

        let (encode_time, buffer) = buffer.decode::<u64>()?;

        let (peer_address, buffer) = decode_address(buffer)?;
        let peer_address =
            peer_address.ok_or(DecoderError::InvariantViolation("Missing peer address"))?;

        let (payload_slice, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;
        let payload = payload_slice.into_less_safe_slice().to_vec();

        let forward = Forward {
            encode_time,
            peer_address,
            payload,
        };

        Ok((forward, buffer))
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        packet::uds::{
            decoder,
            encoder::{self, PACKET_VERSION, PACKET_VERSION_V1, PACKET_VERSION_V2},
            Protocol,
        },
        path::secret::schedule::Ciphersuite,
    };
    use s2n_codec::{DecoderBufferMut, DecoderError, Encoder, EncoderLenEstimator};
    use s2n_quic_core::{dc, inet::SocketAddress};

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

    /// Encodes a v2 packet (estimating the buffer size first) and returns the encoded bytes.
    #[allow(clippy::too_many_arguments)]
    fn encode_packet_v2(
        protocol: Protocol,
        ciphersuite: &Ciphersuite,
        export_secret: &[u8],
        application_params: &dc::ApplicationParams,
        encode_time: u64,
        peer_address: &SocketAddress,
        local_address: Option<&SocketAddress>,
        application_data: Option<&[u8]>,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut estimator = EncoderLenEstimator::new(usize::MAX);
        let expected_size = encoder::encode_v2(
            &mut estimator,
            protocol,
            ciphersuite,
            export_secret,
            application_params,
            encode_time,
            peer_address,
            local_address,
            application_data,
            payload,
        );
        let mut buffer = vec![0u8; expected_size];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut buffer);
        let encoded_size = encoder::encode_v2(
            &mut enc,
            protocol,
            ciphersuite,
            export_secret,
            application_params,
            encode_time,
            peer_address,
            local_address,
            application_data,
            payload,
        );
        assert_eq!(encoded_size, expected_size);
        buffer
    }

    fn socket_address(address: &str) -> SocketAddress {
        address.parse::<std::net::SocketAddr>().unwrap().into()
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
        assert_eq!(packet.protocol(), Protocol::Tcp);
        assert_eq!(packet.peer_address(), None);
        assert_eq!(packet.local_address(), None);
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
        assert_eq!(packet.protocol(), Protocol::Tcp);
        assert_eq!(packet.peer_address(), None);
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
    fn test_encode_decode_v2_udp() {
        let ciphersuite = Ciphersuite::AES_GCM_256_SHA384;
        let export_secret = b"secret_data";
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let peer_address = socket_address("192.0.2.1:4433");
        let local_address = socket_address("[2001:db8::1]:5000");
        let application_data = b"opaque_application_data_blob";
        let payload = b"payload_with_data";
        let time = 456;

        let mut buffer = encode_packet_v2(
            Protocol::Udp,
            &ciphersuite,
            export_secret,
            &application_params,
            time,
            &peer_address,
            Some(&local_address),
            Some(application_data),
            payload,
        );

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION_V2);
        assert_eq!(packet.protocol(), Protocol::Udp);
        assert_eq!(packet.ciphersuite(), ciphersuite);
        assert_eq!(packet.export_secret(), export_secret);
        assert_eq!(packet.encode_time(), time);
        assert_eq!(packet.peer_address(), Some(&peer_address));
        assert_eq!(packet.local_address(), Some(&local_address));
        assert_eq!(packet.application_data(), Some(&application_data[..]));
        assert_eq!(packet.payload(), payload);

        assert_application_params_eq(packet.application_params(), &application_params);
    }

    #[test]
    fn test_encode_decode_v2_tcp_without_optional_fields() {
        let ciphersuite = Ciphersuite::AES_GCM_128_SHA256;
        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let peer_address = socket_address("[2001:db8::2]:443");

        let mut buffer = encode_packet_v2(
            Protocol::Tcp,
            &ciphersuite,
            b"secret",
            &application_params,
            0,
            &peer_address,
            None,
            None,
            b"data",
        );

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION_V2);
        assert_eq!(packet.protocol(), Protocol::Tcp);
        assert_eq!(packet.peer_address(), Some(&peer_address));
        assert_eq!(packet.local_address(), None);
        assert_eq!(packet.application_data(), None);
        assert_eq!(packet.payload(), b"data");
    }

    /// Builds the fields every version shares after the version and protocol bytes.
    fn hand_built_common_fields() -> Vec<u8> {
        let mut buffer = vec![
            0u8, // ciphersuite = 0 (valid)
            4u8, // export secret length = 4
            b't', b'e', b's', b't', // export secret
            0u8,  // application params version = 0
        ];

        let application_params = dc::testing::TEST_APPLICATION_PARAMS;
        let mut params_buf = vec![0u8; 128];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut params_buf);
        enc.encode(&application_params);
        let params_len = enc.len();
        buffer.extend_from_slice(&params_buf[..params_len]);

        // encode_time
        buffer.extend_from_slice(&7u64.to_be_bytes());
        buffer
    }

    /// Pins the v2 wire format: version, protocol, the v0 fields, peer and local addresses,
    /// application data, then payload.
    #[test]
    fn test_decode_hand_built_v2_buffer() {
        let mut buffer = vec![2u8, 1u8]; // version tag = 2, protocol = UDP
        buffer.extend(hand_built_common_fields());
        // peer address: IPv4 192.0.2.1:4433
        buffer.extend_from_slice(&[4, 192, 0, 2, 1]);
        buffer.extend_from_slice(&4433u16.to_be_bytes());
        // local address: none
        buffer.push(0);
        // application data, length-prefixed
        buffer.push(3);
        buffer.extend_from_slice(b"app");
        // payload, length-prefixed
        buffer.push(4);
        buffer.extend_from_slice(b"data");

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (packet, remaining) = decoder::Packet::decode(decoder).unwrap();
        assert!(remaining.is_empty());

        assert_eq!(packet.version_tag(), PACKET_VERSION_V2);
        assert_eq!(packet.protocol(), Protocol::Udp);
        assert_eq!(packet.export_secret(), b"test");
        assert_eq!(packet.encode_time(), 7);
        assert_eq!(
            packet.peer_address(),
            Some(&socket_address("192.0.2.1:4433"))
        );
        assert_eq!(packet.local_address(), None);
        assert_eq!(packet.application_data(), Some(&b"app"[..]));
        assert_eq!(packet.payload(), b"data");
    }

    fn assert_invariant_violation(mut buffer: Vec<u8>, expected: &str) {
        let decoder = DecoderBufferMut::new(&mut buffer);
        match decoder::Packet::decode(decoder) {
            Err(DecoderError::InvariantViolation(msg)) => assert_eq!(msg, expected),
            other => panic!("expected InvariantViolation({expected:?}), got {other:?}"),
        }
    }

    #[test]
    fn test_decode_v2_invalid_protocol() {
        assert_invariant_violation(vec![2u8, 9u8], "Unsupported protocol");
    }

    #[test]
    fn test_decode_v2_invalid_address_family() {
        let mut buffer = vec![2u8, 1u8];
        buffer.extend(hand_built_common_fields());
        buffer.push(5);
        assert_invariant_violation(buffer, "Unsupported address family");
    }

    #[test]
    fn test_decode_v2_missing_peer_address() {
        let mut buffer = vec![2u8, 1u8];
        buffer.extend(hand_built_common_fields());
        buffer.push(0);
        assert_invariant_violation(buffer, "Missing peer address");
    }

    #[test]
    fn test_decode_invalid_version_tag() {
        let mut buffer = vec![4u8, 0u8]; // Unknown version tag = 4
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
    fn test_encode_decode_forward() {
        let peer_address = socket_address("[2001:db8::3]:9000");
        let payload = b"retransmitted packet";

        let mut estimator = EncoderLenEstimator::new(usize::MAX);
        let expected_size = encoder::encode_forward(&mut estimator, 11, &peer_address, payload);
        let mut buffer = vec![0u8; expected_size];
        let mut enc = s2n_codec::EncoderBuffer::new(&mut buffer);
        assert_eq!(
            encoder::encode_forward(&mut enc, 11, &peer_address, payload),
            expected_size
        );

        assert!(decoder::Forward::is_forward(&buffer));
        let decoder = DecoderBufferMut::new(&mut buffer);
        let (forward, remaining) = decoder::Forward::decode(decoder).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(forward.encode_time(), 11);
        assert_eq!(forward.peer_address(), &peer_address);
        assert_eq!(forward.payload(), payload);
    }

    /// Pins the forward wire format: version, encode time, peer address, then payload.
    #[test]
    fn test_decode_hand_built_forward_buffer() {
        let mut buffer = vec![3u8];
        buffer.extend_from_slice(&5u64.to_be_bytes());
        buffer.extend_from_slice(&[4, 10, 0, 0, 1]);
        buffer.extend_from_slice(&80u16.to_be_bytes());
        buffer.push(2);
        buffer.extend_from_slice(b"hi");

        let decoder = DecoderBufferMut::new(&mut buffer);
        let (forward, remaining) = decoder::Forward::decode(decoder).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(forward.encode_time(), 5);
        assert_eq!(forward.peer_address(), &socket_address("10.0.0.1:80"));
        assert_eq!(forward.payload(), b"hi");
    }

    /// A forward is never decoded as a stream handoff, and vice versa.
    #[test]
    fn test_forward_and_handoff_are_distinct() {
        let mut forward = vec![3u8];
        assert_invariant_violation(forward.clone(), "Unsupported version tag");
        forward[0] = 2;
        assert!(!decoder::Forward::is_forward(&forward));
        let decoder = DecoderBufferMut::new(&mut forward);
        assert!(matches!(
            decoder::Forward::decode(decoder),
            Err(DecoderError::InvariantViolation("Unsupported version tag"))
        ));
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
