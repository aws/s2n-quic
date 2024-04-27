// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::check;
use s2n_codec::{assert_codec_round_trip_bytes, assert_codec_round_trip_value};

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn round_trip() {
    check!().for_each(|input| {
        if input.is_empty() {
            return;
        }

        if input[0] > core::u8::MAX / 2 {
            assert_codec_round_trip_bytes!(ClientTransportParameters, input[1..]);
        } else {
            assert_codec_round_trip_bytes!(ServerTransportParameters, input[1..]);
        }
    });
}

macro_rules! default_transport_parameter_test {
    ($endpoint_params:ident) => {
        let default_value = $endpoint_params::default();

        #[cfg(not(miri))] // snapshot tests don't work on miri
        insta::assert_debug_snapshot!(
            concat!(stringify!($endpoint_params), "__default"),
            default_value
        );
        // Tests that a transport parameter will not be sent if it is set
        // to its default value defined in the rfc.
        let encoded_output: Vec<u8> =
            assert_codec_round_trip_value!($endpoint_params, default_value);
        let expected_output: Vec<u8> = vec![];
        assert_eq!(
            encoded_output, expected_output,
            "Default parameters should be empty"
        );
    };
}

#[test]
fn default_server_snapshot_test() {
    default_transport_parameter_test!(ServerTransportParameters);
}

#[test]
fn default_client_snapshot_test() {
    default_transport_parameter_test!(ClientTransportParameters);
}

fn server_transport_parameters() -> ServerTransportParameters {
    // pick a value that isn't the default for any of the params
    let integer_value = VarInt::from_u8(42);

    ServerTransportParameters {
        max_idle_timeout: integer_value.try_into().unwrap(),
        max_udp_payload_size: MaxUdpPayloadSize::new(1500u16).unwrap(),
        initial_max_data: integer_value.try_into().unwrap(),
        initial_max_stream_data_bidi_local: integer_value.try_into().unwrap(),
        initial_max_stream_data_bidi_remote: integer_value.try_into().unwrap(),
        initial_max_stream_data_uni: integer_value.try_into().unwrap(),
        initial_max_streams_bidi: integer_value.try_into().unwrap(),
        initial_max_streams_uni: integer_value.try_into().unwrap(),
        max_datagram_frame_size: MaxDatagramFrameSize::new(0u16).unwrap(),
        ack_delay_exponent: 2u8.try_into().unwrap(),
        max_ack_delay: integer_value.try_into().unwrap(),
        migration_support: MigrationSupport::Disabled,
        active_connection_id_limit: integer_value.try_into().unwrap(),
        original_destination_connection_id: Some([1, 2, 3, 4, 5, 6, 7, 8][..].try_into().unwrap()),
        stateless_reset_token: Some([2; 16].into()),
        preferred_address: Some(PreferredAddress {
            ipv4_address: Some(SocketAddressV4::new([127, 0, 0, 1], 1337)),
            ipv6_address: None,
            connection_id: [4, 5, 6, 7][..].try_into().unwrap(),
            stateless_reset_token: [1; 16].into(),
        }),
        initial_source_connection_id: Some([1, 2, 3, 4][..].try_into().unwrap()),
        retry_source_connection_id: Some([1, 2, 3, 4][..].try_into().unwrap()),
        dc_supported_versions: DcSupportedVersions([
            VarInt::from_u8(5),
            VarInt::from_u8(6),
            VarInt::from_u8(7),
            VarInt::from_u8(8),
        ]),
    }
}

#[test]
fn server_snapshot_test() {
    let value = server_transport_parameters();
    let encoded_output = assert_codec_round_trip_value!(ServerTransportParameters, value);

    #[cfg(not(miri))] // snapshot tests don't work on miri
    insta::assert_debug_snapshot!("server_snapshot_test", encoded_output);

    let _ = encoded_output;
}

fn client_transport_parameters() -> ClientTransportParameters {
    // pick a value that isn't the default for any of the params
    let integer_value = VarInt::from_u8(42);

    ClientTransportParameters {
        max_idle_timeout: integer_value.try_into().unwrap(),
        max_udp_payload_size: MaxUdpPayloadSize::new(1500u16).unwrap(),
        initial_max_data: integer_value.try_into().unwrap(),
        initial_max_stream_data_bidi_local: integer_value.try_into().unwrap(),
        initial_max_stream_data_bidi_remote: integer_value.try_into().unwrap(),
        initial_max_stream_data_uni: integer_value.try_into().unwrap(),
        initial_max_streams_bidi: integer_value.try_into().unwrap(),
        initial_max_streams_uni: integer_value.try_into().unwrap(),
        max_datagram_frame_size: MaxDatagramFrameSize::new(0u16).unwrap(),
        ack_delay_exponent: 2u8.try_into().unwrap(),
        max_ack_delay: integer_value.try_into().unwrap(),
        migration_support: MigrationSupport::Disabled,
        active_connection_id_limit: integer_value.try_into().unwrap(),
        original_destination_connection_id: Default::default(),
        stateless_reset_token: Default::default(),
        preferred_address: Default::default(),
        initial_source_connection_id: Some([1, 2, 3, 4][..].try_into().unwrap()),
        retry_source_connection_id: Default::default(),
        dc_supported_versions: DcSupportedVersions([
            VarInt::from_u8(1),
            VarInt::from_u8(2),
            VarInt::from_u8(3),
            VarInt::from_u8(4),
        ]),
    }
}

#[test]
fn client_snapshot_test() {
    let value = client_transport_parameters();
    let encoded_output = assert_codec_round_trip_value!(ClientTransportParameters, value);

    #[cfg(not(miri))] // snapshot tests don't work on miri
    insta::assert_debug_snapshot!("client_snapshot_test", encoded_output);

    let _ = encoded_output;
}

#[test]
fn load_server_limits() {
    let limits = crate::connection::limits::Limits::default();
    let mut params = ServerTransportParameters::default();
    params.load_limits(&limits);

    #[cfg(not(miri))]
    insta::assert_debug_snapshot!("load_server_limits", params);
}

#[test]
fn load_client_limits() {
    let limits = crate::connection::limits::Limits::default();
    let mut params = ClientTransportParameters::default();
    params.load_limits(&limits);

    #[cfg(not(miri))]
    insta::assert_debug_snapshot!("load_client_limits", params);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-7.4.2
//= type=test
//# An endpoint MUST ignore transport parameters that it does
//# not support.
#[test]
fn ignore_unknown_parameter() {
    use s2n_codec::EncoderBuffer;

    let value = client_transport_parameters();

    // Reserved parameters have tags of the form 31 * N + 27
    // We inject one at the end
    let mut buffer = vec![0; 32 * 1024];
    let mut encoder = EncoderBuffer::new(&mut buffer);

    encoder.encode(&value);

    let id1: TransportParameterId = VarInt::from_u16(31 * 2 + 27);
    encoder.encode(&id1);
    encoder.encode_with_len_prefix::<TransportParameterLength, _>(&());

    let (encoded, _) = encoder.split_off();
    let decoder = DecoderBuffer::new(encoded);
    let (decoded_params, remaining) =
        ClientTransportParameters::decode(decoder).expect("Decoding succeeds");
    assert_eq!(value, decoded_params);
    assert_eq!(0, remaining.len());
}

#[test]
fn compute_data_window_test() {
    assert_eq!(
        *compute_data_window(150, Duration::from_millis(10), 1),
        187_500
    );
    assert_eq!(
        *compute_data_window(150, Duration::from_millis(10), 2),
        375_000
    );
    assert_eq!(
        *compute_data_window(150, Duration::from_millis(100), 2),
        3_750_000
    );
    assert_eq!(
        *compute_data_window(1500, Duration::from_millis(100), 2),
        37_500_000
    );
}

#[test]
fn append_to_buffer() {
    let mut value = client_transport_parameters();

    // Clear the `dc_supported_versions`
    value.dc_supported_versions = DcSupportedVersions::default();

    let versions = [1, 2, 3, 4];

    let mut buffer = value.encode_to_vec();

    // Append `DcSupportedVersions`
    DcSupportedVersions::for_client(versions).append_to_buffer(&mut buffer);

    let decoder = DecoderBuffer::new(&buffer);
    let (mut decoded_params, remaining) =
        ClientTransportParameters::decode(decoder).expect("Decoding succeeds");
    assert_eq!(4, decoded_params.dc_supported_versions.into_iter().len());
    for (index, version) in decoded_params.dc_supported_versions.into_iter().enumerate() {
        assert_eq!(versions[index], version);
    }

    // Clear the `dc_supported_versions` to check the rest of the params
    decoded_params.dc_supported_versions = DcSupportedVersions::default();
    assert_eq!(value, decoded_params);
    assert_eq!(0, remaining.len());
}

#[test]
fn future_larger_supported_versions() {
    use s2n_codec::EncoderBuffer;

    let mut value = client_transport_parameters();

    // Clear the `dc_supported_versions`
    value.dc_supported_versions = DcSupportedVersions::default();

    let mut buffer = vec![0; 32 * 1024];
    let mut encoder = EncoderBuffer::new(&mut buffer);

    encoder.encode(&value);

    encoder.encode(&DcSupportedVersions::ID);
    encoder.encode(&VarInt::from_u8(
        (7 * VarInt::from_u8(1).encoding_size()) as u8,
    ));
    encoder.encode(&VarInt::from_u8(1));
    encoder.encode(&VarInt::from_u8(2));
    encoder.encode(&VarInt::from_u8(3));
    encoder.encode(&VarInt::from_u8(4));
    encoder.encode(&VarInt::from_u8(5));
    encoder.encode(&VarInt::from_u8(6));
    encoder.encode(&VarInt::from_u8(7));

    let (encoded, _) = encoder.split_off();
    let decoder = DecoderBuffer::new(encoded);
    let (decoded_params, remaining) =
        ClientTransportParameters::decode(decoder).expect("Decoding succeeds");
    assert_eq!(
        VarInt::from_u8(1),
        decoded_params.dc_supported_versions.0[0]
    );
    assert_eq!(
        VarInt::from_u8(2),
        decoded_params.dc_supported_versions.0[1]
    );
    assert_eq!(
        VarInt::from_u8(3),
        decoded_params.dc_supported_versions.0[2]
    );
    assert_eq!(
        VarInt::from_u8(4),
        decoded_params.dc_supported_versions.0[3]
    );
    assert_eq!(0, remaining.len());
}

#[test]
fn dc_selected_version() {
    assert_eq!(
        Some(1),
        DcSupportedVersions::for_server(1)
            .selected_version()
            .unwrap()
    );
    assert_eq!(
        None,
        DcSupportedVersions::default().selected_version().unwrap()
    );
    assert!(DcSupportedVersions::for_client([1, 2])
        .selected_version()
        .is_err());
}
