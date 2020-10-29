use crate::{
    connection,
    endpoint::EndpointType,
    inet::{SocketAddressV4, SocketAddressV6, Unspecified},
    stateless_reset_token::StatelessResetToken,
    stream::{StreamId, StreamType},
    varint::VarInt,
};
use core::{
    convert::{TryFrom, TryInto},
    mem::size_of,
    time::Duration,
};
use s2n_codec::{
    decoder_invariant, decoder_value, DecoderBuffer, DecoderBufferMut, DecoderBufferMutResult,
    DecoderBufferResult, DecoderError, DecoderValue, DecoderValueMut, Encoder, EncoderValue,
};

/// Trait for an transport parameter value
pub trait TransportParameter: Sized {
    /// The ID or tag for the TransportParameter
    const ID: TransportParameterId;

    /// Enables/disables the TransportParameter in a certain context
    const ENABLED: bool = true;

    /// Associated type for decoding/encoding the TransportParameter
    type CodecValue;

    /// Create a `TransportParameter` from the CodecValue
    fn from_codec_value(value: Self::CodecValue) -> Self;

    /// Attempts to convert the `TransportParameter` into the `CodecValue`
    fn try_into_codec_value(&self) -> Option<&Self::CodecValue>;

    /// Returns the default value for the TransportParameter
    /// This is used instead of `Default::default` so it is
    /// easily overridable
    fn default_value() -> Self;
}

/// Trait for validating transport parameter values
pub trait TransportParameterValidator: Sized {
    /// Validates that the transport parameter is in a valid state
    fn validate(self) -> Result<Self, DecoderError> {
        Ok(self)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#7.4.1
//# Both endpoints store the value of the server transport parameters
//# from a connection and apply them to any 0-RTT packets that are sent
//# in subsequent connections to that peer, except for transport
//# parameters that are explicitly excluded.  Remembered transport
//# parameters apply to the new connection until the handshake completes
//# and the client starts sending 1-RTT packets.  Once the handshake
//# completes, the client uses the transport parameters established in
//# the handshake.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#7.4.1
//# *  active_connection_id_limit
//# *  initial_max_data
//# *  initial_max_stream_data_bidi_local
//# *  initial_max_stream_data_bidi_remote
//# *  initial_max_stream_data_uni
//# *  initial_max_streams_bidi
//# *  initial_max_streams_uni

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ZeroRTTParameters {
    pub active_connection_id_limit: VarInt,
    pub initial_max_data: VarInt,
    pub initial_max_stream_data_bidi_local: VarInt,
    pub initial_max_stream_data_bidi_remote: VarInt,
    pub initial_max_stream_data_uni: VarInt,
    pub initial_max_streams_bidi: VarInt,
    pub initial_max_streams_uni: VarInt,
}

impl<
        OriginalDestinationConnectionId,
        StatelessResetToken,
        PreferredAddress,
        RetrySourceConnectionId,
    >
    TransportParameters<
        OriginalDestinationConnectionId,
        StatelessResetToken,
        PreferredAddress,
        RetrySourceConnectionId,
    >
{
    /// Returns the ZeroRTTParameters to be saved between connections
    pub fn zero_rtt_parameters(&self) -> ZeroRTTParameters {
        let Self {
            active_connection_id_limit,
            initial_max_data,
            initial_max_stream_data_bidi_local,
            initial_max_stream_data_bidi_remote,
            initial_max_stream_data_uni,
            initial_max_streams_bidi,
            initial_max_streams_uni,
            ..
        } = self;
        ZeroRTTParameters {
            active_connection_id_limit: **active_connection_id_limit,
            initial_max_data: **initial_max_data,
            initial_max_stream_data_bidi_local: **initial_max_stream_data_bidi_local,
            initial_max_stream_data_bidi_remote: **initial_max_stream_data_bidi_remote,
            initial_max_stream_data_uni: **initial_max_stream_data_uni,
            initial_max_streams_bidi: **initial_max_streams_bidi,
            initial_max_streams_uni: **initial_max_streams_uni,
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18
//# The extension_data field of the quic_transport_parameters extension
//# defined in [QUIC-TLS] contains the QUIC transport parameters.  They
//# are encoded as a sequence of transport parameters, as shown in
//# Figure 19:
//#
//# Transport Parameters {
//#   Transport Parameter (..) ...,
//# }
//#                Figure 19: Sequence of Transport Parameters

decoder_value!(
    impl<'a> ClientTransportParameters {
        fn decode(buffer: Buffer) -> Result<Self> {
            let len = buffer.len();
            let (slice, buffer) = buffer.decode_slice(len)?;
            let parameters = Self::decode_parameters(slice.peek())?;
            Ok((parameters, buffer))
        }
    }
);

decoder_value!(
    impl<'a> ServerTransportParameters {
        fn decode(buffer: Buffer) -> Result<Self> {
            let len = buffer.len();
            let (slice, buffer) = buffer.decode_slice(len)?;
            let parameters = Self::decode_parameters(slice.peek())?;
            Ok((parameters, buffer))
        }
    }
);

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18
//# Transport Parameter {
//#   Transport Parameter ID (i),
//#   Transport Parameter Length (i),
//#   Transport Parameter Value (..),
//# }
//#
//#                Figure 20: Transport Parameter Encoding
//#
//# The Transport Parameter Length field contains the length of the
//# Transport Parameter Value field.
//#
//# QUIC encodes transport parameters into a sequence of bytes, which are
//# then included in the cryptographic handshake.

type TransportParameterId = VarInt;
type TransportParameterLength = VarInt;

/// Utility struct for encoding and decoding transport parameters
struct TransportParameterCodec<T>(T);

impl<'a, T: TransportParameter> DecoderValue<'a> for TransportParameterCodec<T>
where
    T::CodecValue: DecoderValue<'a>,
{
    fn decode(buffer: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self> {
        let (value, buffer) = buffer.decode_with_len_prefix::<TransportParameterLength, _>()?;
        Ok((Self(T::from_codec_value(value)), buffer))
    }
}

impl<'a, T: TransportParameter> DecoderValueMut<'a> for TransportParameterCodec<T>
where
    T::CodecValue: DecoderValueMut<'a>,
{
    fn decode_mut(buffer: DecoderBufferMut<'a>) -> DecoderBufferMutResult<'a, Self> {
        let (value, buffer) = buffer.decode_with_len_prefix::<TransportParameterLength, _>()?;
        Ok((Self(T::from_codec_value(value)), buffer))
    }
}

impl<T: TransportParameter> EncoderValue for TransportParameterCodec<&T>
where
    T::CodecValue: EncoderValue,
{
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        if let Some(value) = self.0.try_into_codec_value() {
            buffer.encode(&T::ID);
            buffer.encode_with_len_prefix::<TransportParameterLength, _>(value);
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ValidationError(&'static str);

impl core::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<DecoderError> for ValidationError {
    fn from(error: DecoderError) -> Self {
        ValidationError(error.into())
    }
}

/// Creates a transport parameter struct with the inner codec type
macro_rules! transport_parameter {
    ($name:ident($encodable_type:ty), $tag:expr) => {
        transport_parameter!(
            $name($encodable_type),
            $tag,
            <$encodable_type as Default>::default()
        );
    };
    ($name:ident($encodable_type:ty), $tag:expr, $default:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord)]
        pub struct $name($encodable_type);

        impl Default for $name {
            fn default() -> Self {
                Self($default)
            }
        }

        impl $name {
            // Create a transport parameter with the given value
            pub fn new<T: TryInto<$encodable_type>>(value: T) -> Option<Self> {
                value
                    .try_into()
                    .ok()
                    .map(Self)
                    .and_then(|value| value.validate().ok())
            }
        }

        impl TryFrom<$encodable_type> for $name {
            type Error = ValidationError;

            fn try_from(value: $encodable_type) -> Result<Self, Self::Error> {
                Self(value).validate().map_err(|err| err.into())
            }
        }

        impl TransportParameter for $name {
            type CodecValue = $encodable_type;

            const ID: TransportParameterId = TransportParameterId::from_u8($tag);

            fn from_codec_value(value: Self::CodecValue) -> Self {
                Self(value)
            }

            fn try_into_codec_value(&self) -> Option<&Self::CodecValue> {
                if self.0 == $default {
                    None
                } else {
                    Some(&self.0)
                }
            }

            fn default_value() -> Self {
                Self($default)
            }
        }

        impl core::ops::Deref for $name {
            type Target = $encodable_type;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl PartialEq<$encodable_type> for $name {
            fn eq(&self, value: &$encodable_type) -> bool {
                self.0.eq(value)
            }
        }

        impl PartialOrd<$encodable_type> for $name {
            fn partial_cmp(&self, value: &$encodable_type) -> Option<core::cmp::Ordering> {
                self.0.partial_cmp(value)
            }
        }
    };
}

/// Implements an optional transport parameter
macro_rules! optional_transport_parameter {
    ($ty:ty) => {
        impl TransportParameter for Option<$ty> {
            type CodecValue = $ty;

            const ID: TransportParameterId = <$ty as TransportParameter>::ID;

            fn from_codec_value(value: Self::CodecValue) -> Self {
                Some(value)
            }

            fn try_into_codec_value(&self) -> Option<&Self::CodecValue> {
                self.as_ref()
            }

            fn default_value() -> Self {
                None
            }
        }

        impl TransportParameterValidator for Option<$ty> {
            fn validate(self) -> Result<Self, DecoderError> {
                if let Some(value) = self {
                    Ok(Some(value.validate()?))
                } else {
                    Ok(None)
                }
            }
        }
    };
}

macro_rules! connection_id_parameter {
    ($name:ident, $tag:expr) => {
        transport_parameter!($name(connection::Id), $tag, connection::Id::EMPTY);

        // The inner connection_id handles validation
        impl TransportParameterValidator for $name {}

        impl TryFrom<&[u8]> for $name {
            type Error = crate::connection::id::Error;

            fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self(connection::Id::try_from(value)?))
            }
        }

        decoder_value!(
            impl<'a> $name {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (connection_id, buffer) = buffer.decode()?;
                    Ok((Self(connection_id), buffer))
                }
            }
        );

        impl EncoderValue for $name {
            fn encode<E: Encoder>(&self, encoder: &mut E) {
                self.0.encode(encoder)
            }
        }
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# original_destination_connection_id (0x00): The value of the
//#    Destination Connection ID field from the first Initial packet sent
//#    by the client; see Section 7.3.  This transport parameter is only
//#    sent by a server.

connection_id_parameter!(OriginalDestinationConnectionId, 0x00);

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# max_idle_timeout (0x01):  The max idle timeout is a value in
//#    milliseconds that is encoded as an integer; see (Section 10.2).
//#    Idle timeout is disabled when both endpoints omit this transport
//#    parameter or specify a value of 0.

transport_parameter!(MaxIdleTimeout(VarInt), 0x01);

impl TransportParameterValidator for MaxIdleTimeout {}

impl MaxIdleTimeout {
    /// Try to convert `core::time::Duration` into an idle_timeout transport parameter
    pub fn try_from_duration(value: Duration) -> Option<Self> {
        let value: u64 = value.as_millis().try_into().ok()?;
        Self::new(value)
    }

    /// Convert idle_timeout into a `core::time::Duration`
    pub const fn as_duration(self) -> Duration {
        Duration::from_millis(self.0.as_u64())
    }
}

impl TryFrom<Duration> for MaxIdleTimeout {
    type Error = ValidationError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        Self::try_from_duration(value).ok_or(ValidationError("Duration exceeds encodable limit"))
    }
}

impl Into<Duration> for MaxIdleTimeout {
    fn into(self) -> Duration {
        self.as_duration()
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# stateless_reset_token (0x02):  A stateless reset token is used in
//#    verifying a stateless reset; see Section 10.4.  This parameter is
//#    a sequence of 16 bytes.  This transport parameter MUST NOT be sent
//#    by a client, but MAY be sent by a server.  A server that does not
//#    send this transport parameter cannot use stateless reset
//#    (Section 10.4) for the connection ID negotiated during the
//#    handshake.

optional_transport_parameter!(StatelessResetToken);

impl TransportParameter for StatelessResetToken {
    type CodecValue = Self;

    const ID: TransportParameterId = TransportParameterId::from_u8(0x02);

    fn from_codec_value(value: Self) -> Self {
        value
    }

    fn try_into_codec_value(&self) -> Option<&Self> {
        Some(self)
    }

    fn default_value() -> Self {
        Self::ZEROED
    }
}

impl TransportParameterValidator for StatelessResetToken {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# max_udp_payload_size (0x03):  The maximum UDP payload size parameter
//#    is an integer value that limits the size of UDP payloads that the
//#    endpoint is willing to receive.  UDP packets with payloads larger
//#    than this limit are not likely to be processed by the receiver.
//#
//#    The default for this parameter is the maximum permitted UDP
//#    payload of 65527.  Values below 1200 are invalid.
//#
//#    This limit does act as an additional constraint on datagram size
//#    in the same way as the path MTU, but it is a property of the
//#    endpoint and not the path; see Section 14.  It is expected that
//#    this is the space an endpoint dedicates to holding incoming
//#    packets.

transport_parameter!(MaxUdpPayloadSize(VarInt), 0x03, VarInt::from_u16(65527));

impl TransportParameterValidator for MaxUdpPayloadSize {
    fn validate(self) -> Result<Self, DecoderError> {
        decoder_invariant!(
            (1200..=65527).contains(&*self.0),
            "max_packet_size should be within 1200 and 65527 bytes"
        );
        Ok(self)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_data (0x04):  The initial maximum data parameter is an
//#    integer value that contains the initial value for the maximum
//#    amount of data that can be sent on the connection.  This is
//#    equivalent to sending a MAX_DATA (Section 19.9) for the connection
//#    immediately after completing the handshake.

transport_parameter!(InitialMaxData(VarInt), 0x04);

impl TransportParameterValidator for InitialMaxData {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_stream_data_bidi_local (0x05):  This parameter is an
//#    integer value specifying the initial flow control limit for
//#    locally-initiated bidirectional streams.  This limit applies to
//#    newly created bidirectional streams opened by the endpoint that
//#    sends the transport parameter.  In client transport parameters,
//#    this applies to streams with an identifier with the least
//#    significant two bits set to 0x0; in server transport parameters,
//#    this applies to streams with the least significant two bits set to
//#    0x1.

transport_parameter!(InitialMaxStreamDataBidiLocal(VarInt), 0x05);

impl TransportParameterValidator for InitialMaxStreamDataBidiLocal {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_stream_data_bidi_remote (0x06):  This parameter is an
//#    integer value specifying the initial flow control limit for peer-
//#    initiated bidirectional streams.  This limit applies to newly
//#    created bidirectional streams opened by the endpoint that receives
//#    the transport parameter.  In client transport parameters, this
//#    applies to streams with an identifier with the least significant
//#    two bits set to 0x1; in server transport parameters, this applies
//#    to streams with the least significant two bits set to 0x0.

transport_parameter!(InitialMaxStreamDataBidiRemote(VarInt), 0x06);

impl TransportParameterValidator for InitialMaxStreamDataBidiRemote {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_stream_data_uni (0x07):  This parameter is an integer
//#    value specifying the initial flow control limit for unidirectional
//#    streams.  This limit applies to newly created unidirectional
//#    streams opened by the endpoint that receives the transport
//#    parameter.  In client transport parameters, this applies to
//#    streams with an identifier with the least significant two bits set
//#    to 0x3; in server transport parameters, this applies to streams
//#    with the least significant two bits set to 0x2.

transport_parameter!(InitialMaxStreamDataUni(VarInt), 0x07);

impl TransportParameterValidator for InitialMaxStreamDataUni {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_streams_bidi (0x08):  The initial maximum bidirectional
//#    streams parameter is an integer value that contains the initial
//#    maximum number of bidirectional streams the peer may initiate.  If
//#    this parameter is absent or zero, the peer cannot open
//#    bidirectional streams until a MAX_STREAMS frame is sent.  Setting
//#    this parameter is equivalent to sending a MAX_STREAMS
//#    (Section 19.11) of the corresponding type with the same value.

transport_parameter!(InitialMaxStreamsBidi(VarInt), 0x08);

impl TransportParameterValidator for InitialMaxStreamsBidi {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_max_streams_uni (0x09):  The initial maximum unidirectional
//#    streams parameter is an integer value that contains the initial
//#    maximum number of unidirectional streams the peer may initiate.
//#    If this parameter is absent or zero, the peer cannot open
//#    unidirectional streams until a MAX_STREAMS frame is sent.  Setting
//#    this parameter is equivalent to sending a MAX_STREAMS
//#    (Section 19.11) of the corresponding type with the same value.

transport_parameter!(InitialMaxStreamsUni(VarInt), 0x09);

impl TransportParameterValidator for InitialMaxStreamsUni {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# ack_delay_exponent (0x0a):  The ACK delay exponent is an integer
//#    value indicating an exponent used to decode the ACK Delay field in
//#    the ACK frame (Section 19.3).  If this value is absent, a default
//#    value of 3 is assumed (indicating a multiplier of 8).  Values
//#    above 20 are invalid.

transport_parameter!(AckDelayExponent(u8), 0x0a, 3);

impl TransportParameterValidator for AckDelayExponent {
    fn validate(self) -> Result<Self, DecoderError> {
        decoder_invariant!(self.0 <= 20, "ack_delay_exponent cannot be greater than 20");
        Ok(self)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# max_ack_delay (0x0b):  The maximum ACK delay is an integer value
//#    indicating the maximum amount of time in milliseconds by which the
//#    endpoint will delay sending acknowledgments.  This value SHOULD
//#    include the receiver's expected delays in alarms firing.  For
//#    example, if a receiver sets a timer for 5ms and alarms commonly
//#    fire up to 1ms late, then it should send a max_ack_delay of 6ms.
//#    If this value is absent, a default of 25 milliseconds is assumed.
//#    Values of 2^14 or greater are invalid.

transport_parameter!(MaxAckDelay(VarInt), 0x0b, VarInt::from_u8(25));

impl TransportParameterValidator for MaxAckDelay {
    fn validate(self) -> Result<Self, DecoderError> {
        decoder_invariant!(
            *self.0 <= 2u64.pow(14),
            "max_ack_delay cannot be greater than 2^14"
        );
        Ok(self)
    }
}

impl MaxAckDelay {
    /// Try to convert `core::time::Duration` into a max_ack_delay transport parameter
    pub fn try_from_duration(value: Duration) -> Option<Self> {
        let value: u64 = value.as_millis().try_into().ok()?;
        Self::new(value)
    }

    /// Convert max_ack_delay into a `core::time::Duration`
    pub const fn as_duration(self) -> Duration {
        Duration::from_millis(self.0.as_u64())
    }
}

impl TryFrom<Duration> for MaxAckDelay {
    type Error = ValidationError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        Self::try_from_duration(value).ok_or(ValidationError("Duration exceeds limit of 2^14ms"))
    }
}

impl Into<Duration> for MaxAckDelay {
    fn into(self) -> Duration {
        self.as_duration()
    }
}

/// Settings for ACK frames
#[derive(Clone, Copy, Debug)]
pub struct AckSettings {
    /// The maximum ACK delay indicates the maximum amount of time by which the
    /// endpoint will delay sending acknowledgments.
    pub max_ack_delay: Duration,
    /// The ACK delay exponent is an integer value indicating an exponent used
    /// to decode the ACK Delay field in the ACK frame
    pub ack_delay_exponent: u8,
}

impl Default for AckSettings {
    fn default() -> Self {
        Self {
            max_ack_delay: MaxAckDelay::default_value().into(),
            ack_delay_exponent: *AckDelayExponent::default_value(),
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.3
//# A variable-length integer representing the time delta in
//# microseconds between when this ACK was sent and when the largest
//# acknowledged packet, as indicated in the Largest Acknowledged
//# field, was received by this peer.  The value of the ACK Delay
//# field is scaled by multiplying the encoded value by 2 to the power
//# of the value of the ack_delay_exponent transport parameter set by
//# the sender of the ACK frame; see Section 18.2.  Scaling in this
//# fashion allows for a larger range of values with a shorter
//# encoding at the cost of lower resolution.  Because the receiver
//# doesn't use the ACK Delay for Initial and Handshake packets, a
//# sender SHOULD send a value of 0.

impl AckSettings {
    /// Decodes the peer's `Ack Delay` field
    pub fn decode_ack_delay(&self, delay: VarInt) -> Duration {
        Duration::from_micros(*delay) * self.scale()
    }

    /// Encodes the local `Ack Delay` field
    pub fn encode_ack_delay(&self, delay: Duration) -> VarInt {
        let micros = delay.as_micros();
        let scale = self.scale() as u128;
        (micros / scale).try_into().unwrap_or(VarInt::MAX)
    }

    /// Computes the scale from the exponent
    fn scale(&self) -> u32 {
        2u32.pow(self.ack_delay_exponent as u32)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//#  retry_source_connection_id (0x10):  The value that the server
//#     included in the Source Connection ID field of a Retry packet; see
//#     Section 7.3.  This transport parameter is only sent by a server.
// optional_transport_parameter!(RetrySourceConnectionId, 0x10);

#[cfg(test)]
mod ack_settings_tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri
    fn ack_settings_test() {
        for ack_delay_exponent in 0..=20 {
            let settings = AckSettings {
                max_ack_delay: Default::default(),
                ack_delay_exponent,
            };
            // use an epsilon instead of comparing the values directly,
            // as there will be some precision loss
            let epsilon = settings.scale() as u128;

            for delay in (0..1000).map(|v| v * 100).map(Duration::from_micros) {
                let delay_varint = settings.encode_ack_delay(delay);
                let expected_us = delay.as_micros();
                let actual_us = settings.decode_ack_delay(delay_varint).as_micros();
                let actual_difference = expected_us - actual_us;
                assert!(actual_difference < epsilon);
            }

            // ensure MAX values are handled correctly and don't overflow
            let delay = settings.decode_ack_delay(VarInt::MAX);
            let delay_varint = settings.encode_ack_delay(delay);
            assert_eq!(VarInt::MAX, delay_varint);
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# disable_active_migration (0x0c): The disable active migration
//#    transport parameter is included if the endpoint does not support
//#    active connection migration (Section 9) on the address being used
//#    during the handshake.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MigrationSupport {
    Enabled,
    Disabled,
}

impl Default for MigrationSupport {
    fn default() -> Self {
        MigrationSupport::Enabled
    }
}

impl TransportParameter for MigrationSupport {
    type CodecValue = ();

    const ID: TransportParameterId = TransportParameterId::from_u8(0x0c);

    fn from_codec_value(_value: ()) -> Self {
        MigrationSupport::Disabled
    }

    fn try_into_codec_value(&self) -> Option<&()> {
        if let MigrationSupport::Disabled = self {
            Some(&())
        } else {
            None
        }
    }

    fn default_value() -> Self {
        MigrationSupport::Enabled
    }
}

impl TransportParameterValidator for MigrationSupport {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# preferred_address (0x0d):  The server's preferred address is used to
//#    effect a change in server address at the end of the handshake, as
//#    described in Section 9.6.  The format of this transport parameter
//#    is shown in Figure 21.  This transport parameter is only sent by a
//#    server.  Servers MAY choose to only send a preferred address of
//#    one address family by sending an all-zero address and port
//#    (0.0.0.0:0 or ::.0) for the other family.  IP addresses are
//#    encoded in network byte order.

optional_transport_parameter!(PreferredAddress);

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# Preferred Address {
//#   IPv4 Address (32),
//#   IPv4 Port (16),
//#   IPv6 Address (128),
//#   IPv6 Port (16),
//#   CID Length (8),
//#   Connection ID (..),
//#   Stateless Reset Token (128),
//# }
//#
//#                  Figure 21: Preferred Address format

type CIDLength = u8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreferredAddress {
    pub ipv4_address: Option<SocketAddressV4>,
    pub ipv6_address: Option<SocketAddressV6>,
    pub connection_id: crate::connection::Id,
    pub stateless_reset_token: crate::stateless_reset_token::StatelessResetToken,
}

impl Unspecified for PreferredAddress {
    fn is_unspecified(&self) -> bool {
        self.ipv4_address
            .as_ref()
            .map(Unspecified::is_unspecified)
            .unwrap_or(true)
            && self
                .ipv6_address
                .as_ref()
                .map(Unspecified::is_unspecified)
                .unwrap_or(true)
    }
}

impl TransportParameter for PreferredAddress {
    type CodecValue = Self;

    const ID: TransportParameterId = TransportParameterId::from_u8(0x0d);

    fn from_codec_value(value: Self) -> Self {
        value
    }

    fn try_into_codec_value(&self) -> Option<&Self> {
        Some(self)
    }

    fn default_value() -> Self {
        Self {
            ipv4_address: None,
            ipv6_address: None,
            connection_id: connection::Id::EMPTY,
            stateless_reset_token: StatelessResetToken::ZEROED,
        }
    }
}

impl TransportParameterValidator for PreferredAddress {
    fn validate(self) -> Result<Self, DecoderError> {
        decoder_invariant!(
            !self.is_unspecified(),
            "at least one address needs to be specified"
        );
        Ok(self)
    }
}

decoder_value!(
    impl<'a> PreferredAddress {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (ipv4_address, buffer) = buffer.decode::<SocketAddressV4>()?;
            let ipv4_address = ipv4_address.filter_unspecified();
            let (ipv6_address, buffer) = buffer.decode::<SocketAddressV6>()?;
            let ipv6_address = ipv6_address.filter_unspecified();
            let (connection_id, buffer) = buffer.decode_with_len_prefix::<CIDLength, _>()?;
            let (stateless_reset_token, buffer) = buffer.decode()?;
            let preferred_address = Self {
                ipv4_address,
                ipv6_address,
                connection_id,
                stateless_reset_token,
            };
            Ok((preferred_address, buffer))
        }
    }
);

impl EncoderValue for PreferredAddress {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        if let Some(ip) = self.ipv4_address.as_ref() {
            buffer.encode(ip);
        } else {
            buffer.write_repeated(size_of::<SocketAddressV4>(), 0);
        }

        if let Some(ip) = self.ipv6_address.as_ref() {
            buffer.encode(ip);
        } else {
            buffer.write_repeated(size_of::<SocketAddressV6>(), 0);
        }
        buffer.encode_with_len_prefix::<CIDLength, _>(&self.connection_id);
        buffer.encode(&self.stateless_reset_token);
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# active_connection_id_limit (0x0e):  The active connection ID limit is
//#    an integer value specifying the maximum number of connection IDs
//#    from the peer that an endpoint is willing to store.  This value
//#    includes the connection ID received during the handshake, that
//#    received in the preferred_address transport parameter, and those
//#    received in NEW_CONNECTION_ID frames.  The value of the
//#    active_connection_id_limit parameter MUST be at least 2.  An
//#    endpoint that receives a value less than 2 MUST close the
//#    connection with an error of type TRANSPORT_PARAMETER_ERROR.  If
//#    this transport parameter is absent, a default of 2 is assumed.  If
//#    an endpoint issues a zero-length connection ID, it will never send
//#    a NEW_CONNECTION_ID frame and therefore ignores the
//#    active_connection_id_limit value received from its peer.

transport_parameter!(ActiveConnectionIdLimit(VarInt), 0x0e, VarInt::from_u8(2));

impl TransportParameterValidator for ActiveConnectionIdLimit {
    fn validate(self) -> Result<Self, DecoderError> {
        decoder_invariant!(
            *self.0 >= 2,
            "active_connection_id_limit must be at least 2"
        );
        Ok(self)
    }
}

impl ActiveConnectionIdLimit {
    /// Returns true if the specified value is the default
    pub fn is_default(self) -> bool {
        self == Self::default_value()
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# initial_source_connection_id (0x0f):  The value that the endpoint
//#    included in the Source Connection ID field of the first Initial
//#    packet it sends for the connection; see Section 7.3.

connection_id_parameter!(InitialSourceConnectionId, 0x0f);

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# retry_source_connection_id (0x10):  The value that the server
//#    included in the Source Connection ID field of a Retry packet; see
//#    Section 7.3.  This transport parameter is only sent by a server.

connection_id_parameter!(RetrySourceConnectionId, 0x10);
optional_transport_parameter!(RetrySourceConnectionId);

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# If present, transport parameters that set initial flow control limits
//# (initial_max_stream_data_bidi_local,
//# initial_max_stream_data_bidi_remote, and initial_max_stream_data_uni)
//# are equivalent to sending a MAX_STREAM_DATA frame (Section 19.10) on
//# every stream of the corresponding type immediately after opening.  If
//# the transport parameter is absent, streams of that type start with a
//# flow control limit of 0.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InitialFlowControlLimits {
    pub stream_limits: InitialStreamLimits,
    pub max_data: VarInt,
    pub max_streams_bidi: VarInt,
    pub max_streams_uni: VarInt,
}

/// Associated flow control limits from a set of TransportParameters
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InitialStreamLimits {
    pub max_data_bidi_local: VarInt,
    pub max_data_bidi_remote: VarInt,
    pub max_data_uni: VarInt,
}

impl InitialStreamLimits {
    /// Returns the initial maximum data limit for a Stream based on its Stream ID
    /// and the information whether the "local" endpoint is referring to the Client
    /// or the Server.
    pub fn max_data(&self, local_endpoint_type: EndpointType, stream_id: StreamId) -> VarInt {
        match (stream_id.initiator(), stream_id.stream_type()) {
            (endpoint_type, StreamType::Bidirectional) if endpoint_type == local_endpoint_type => {
                self.max_data_bidi_local
            }
            (_, StreamType::Bidirectional) => self.max_data_bidi_remote,
            (_, StreamType::Unidirectional) => self.max_data_uni,
        }
    }
}

impl<
        OriginalDestinationConnectionId,
        StatelessResetToken,
        PreferredAddress,
        RetrySourceConnectionId,
    >
    TransportParameters<
        OriginalDestinationConnectionId,
        StatelessResetToken,
        PreferredAddress,
        RetrySourceConnectionId,
    >
{
    /// Returns the flow control limits from a set of TransportParameters
    pub fn flow_control_limits(&self) -> InitialFlowControlLimits {
        let Self {
            initial_max_data,
            initial_max_streams_bidi,
            initial_max_streams_uni,
            ..
        } = self;
        InitialFlowControlLimits {
            stream_limits: self.stream_limits(),
            max_data: **initial_max_data,
            max_streams_bidi: **initial_max_streams_bidi,
            max_streams_uni: **initial_max_streams_uni,
        }
    }

    /// Returns the flow control limits from a set of TransportParameters
    pub fn stream_limits(&self) -> InitialStreamLimits {
        let Self {
            initial_max_stream_data_bidi_local,
            initial_max_stream_data_bidi_remote,
            initial_max_stream_data_uni,
            ..
        } = self;
        InitialStreamLimits {
            max_data_bidi_local: **initial_max_stream_data_bidi_local,
            max_data_bidi_remote: **initial_max_stream_data_bidi_remote,
            max_data_uni: **initial_max_stream_data_uni,
        }
    }

    // Returns the AckSettings from a set of TransportParameters
    pub fn ack_settings(&self) -> AckSettings {
        let Self {
            max_ack_delay,
            ack_delay_exponent,
            ..
        } = self;
        AckSettings {
            max_ack_delay: max_ack_delay.as_duration(),
            ack_delay_exponent: **ack_delay_exponent,
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.2
//# A client MUST NOT include any server-only transport parameter:
//# original_destination_connection_id, preferred_address,
//# retry_source_connection_id, or stateless_reset_token.  A server MUST
//# treat receipt of any of these transport parameters as a connection
//# error of type TRANSPORT_PARAMETER_ERROR.

mod disabled_parameter;
pub use disabled_parameter::DisabledParameter;

/// Specific TransportParameters sent by the client endpoint
pub type ClientTransportParameters = TransportParameters<
    DisabledParameter<OriginalDestinationConnectionId>,
    DisabledParameter<StatelessResetToken>,
    DisabledParameter<PreferredAddress>,
    DisabledParameter<RetrySourceConnectionId>,
>;

/// Specific TransportParameters sent by the server endpoint
pub type ServerTransportParameters = TransportParameters<
    OriginalDestinationConnectionId,
    Option<StatelessResetToken>,
    Option<PreferredAddress>,
    Option<RetrySourceConnectionId>,
>;

macro_rules! impl_transport_parameters {
    (
        pub struct TransportParameters <
        $($server_param:ident),* $(,)? >
        { $($field:ident : $field_ty:ident),* $(,)? }
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct TransportParameters<$($server_param),*> {
            $(
                pub $field: $field_ty
            ),*
        }

        impl<'a, $($server_param),*> Default for TransportParameters<$($server_param),*>
        where
            $(
                $server_param: TransportParameter,
            )*
        {
            fn default() -> Self {
                Self {
                    $(
                        $field: TransportParameter::default_value(),
                    )*
                }
            }
        }

        impl<'a, $($server_param),*> EncoderValue for TransportParameters<$($server_param),*>
        where
            $(
                $server_param: TransportParameter,
                $server_param::CodecValue: EncoderValue,
            )*
        {
            fn encode<E: Encoder>(&self, buffer: &mut E) {
                $(
                    buffer.encode(&TransportParameterCodec(&self.$field));
                )*
            }
        }

        impl<'a, $($server_param),*> TransportParameters<$($server_param),*>
        where
            $(
                $server_param: TransportParameter + TransportParameterValidator,
                $server_param::CodecValue: DecoderValue<'a>,
            )*
        {
            fn decode_parameters(
                mut buffer: DecoderBuffer<'a>
            ) -> Result<TransportParameters<$($server_param),*>, DecoderError> {
                let mut parameters = Self::default();

                /// Tracks the fields for duplicates
                #[derive(Default)]
                struct UsedFields {
                    $(
                        $field: bool,
                    )*
                }

                let mut used_fields = UsedFields::default();

                while !buffer.is_empty() {
                    let (tag, inner_buffer) = buffer.decode::<TransportParameterId>()?;

                    buffer = match tag {
                        $(
                            tag if tag == $field_ty::ID => {
                                // ensure the field is enabled in this context
                                s2n_codec::decoder_invariant!(
                                    $field_ty::ENABLED,
                                    concat!(stringify!($field), " is not allowed in this context")
                                );

                                s2n_codec::decoder_invariant!(
                                    core::mem::replace(&mut used_fields.$field, true) == false,
                                    concat!("duplicate value for ", stringify!($field))
                                );
                                let (value, inner_buffer) =
                                    inner_buffer.decode::<TransportParameterCodec<$field_ty>>()?;
                                parameters.$field = value.0.validate()?;
                                inner_buffer
                            }
                        )*
                        _ => {
                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#7.4.2
                            //# New transport parameters can be used to negotiate new protocol
                            //# behavior.  An endpoint MUST ignore transport parameters that it does
                            //# not support.  Absence of a transport parameter therefore disables any
                            //# optional protocol feature that is negotiated using the parameter.  As
                            //# described in Section 18.1, some identifiers are reserved in order to
                            //# exercise this requirement.

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#18.1
                            //# Transport parameters with an identifier of the form "31 * N + 27" for
                            //# integer values of N are reserved to exercise the requirement that
                            //# unknown transport parameters be ignored.  These transport parameters
                            //# have no semantics, and may carry arbitrary values.

                            // ignore transport parameters with unknown tags
                            // We need to skip the actual content of the parameters, which
                            // consists of a VarInt length field plus payload
                            inner_buffer.skip_with_len_prefix::<TransportParameterLength>()?
                        }
                    }
                }

                Ok(parameters)
            }
        }
    };
}

impl_transport_parameters!(
    pub struct TransportParameters<
        OriginalDestinationConnectionId,
        StatelessResetToken,
        PreferredAddress,
        RetrySourceConnectionId,
    > {
        max_idle_timeout: MaxIdleTimeout,
        max_udp_payload_size: MaxUdpPayloadSize,
        initial_max_data: InitialMaxData,
        initial_max_stream_data_bidi_local: InitialMaxStreamDataBidiLocal,
        initial_max_stream_data_bidi_remote: InitialMaxStreamDataBidiRemote,
        initial_max_stream_data_uni: InitialMaxStreamDataUni,
        initial_max_streams_bidi: InitialMaxStreamsBidi,
        initial_max_streams_uni: InitialMaxStreamsUni,
        ack_delay_exponent: AckDelayExponent,
        max_ack_delay: MaxAckDelay,
        migration_support: MigrationSupport,
        active_connection_id_limit: ActiveConnectionIdLimit,
        original_destination_connection_id: OriginalDestinationConnectionId,
        stateless_reset_token: StatelessResetToken,
        preferred_address: PreferredAddress,
        initial_source_connection_id: InitialSourceConnectionId,
        retry_source_connection_id: RetrySourceConnectionId,
    }
);

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use s2n_codec::assert_codec_round_trip_value;

    #[test]
    fn default_snapshot_test() {
        macro_rules! default_transport_parameter_test {
            ($endpoint_params:ident) => {
                let default_value = $endpoint_params::default();

                #[cfg(not(miri))] // snapshot tests don't work on miri
                insta::assert_debug_snapshot!(
                    concat!(stringify!($endpoint_params), " default"),
                    default_value
                );

                let encoded_output: Vec<u8> =
                    assert_codec_round_trip_value!($endpoint_params, default_value);
                let expected_output: Vec<u8> = vec![];
                assert_eq!(
                    encoded_output, expected_output,
                    "Default parameters should be empty"
                );
            };
        }

        default_transport_parameter_test!(ClientTransportParameters);
        default_transport_parameter_test!(ServerTransportParameters);
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
            ack_delay_exponent: 2u8.try_into().unwrap(),
            max_ack_delay: integer_value.try_into().unwrap(),
            migration_support: MigrationSupport::Disabled,
            active_connection_id_limit: integer_value.try_into().unwrap(),
            original_destination_connection_id: [1, 2, 3][..].try_into().unwrap(),
            stateless_reset_token: Some([2; 16].into()),
            preferred_address: Some(PreferredAddress {
                ipv4_address: Some(SocketAddressV4::new([127, 0, 0, 1], 1337)),
                ipv6_address: None,
                connection_id: [4, 5, 6, 7][..].try_into().unwrap(),
                stateless_reset_token: [1; 16].into(),
            }),
            initial_source_connection_id: [1, 2, 3][..].try_into().unwrap(),
            retry_source_connection_id: Some([1, 2, 3][..].try_into().unwrap()),
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

    fn client_transport_paramters() -> ClientTransportParameters {
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
            ack_delay_exponent: 2u8.try_into().unwrap(),
            max_ack_delay: integer_value.try_into().unwrap(),
            migration_support: MigrationSupport::Disabled,
            active_connection_id_limit: integer_value.try_into().unwrap(),
            original_destination_connection_id: Default::default(),
            stateless_reset_token: Default::default(),
            preferred_address: Default::default(),
            initial_source_connection_id: [1, 2, 3][..].try_into().unwrap(),
            retry_source_connection_id: Default::default(),
        }
    }

    #[test]
    fn client_snapshot_test() {
        let value = client_transport_paramters();
        let encoded_output = assert_codec_round_trip_value!(ClientTransportParameters, value);

        #[cfg(not(miri))] // snapshot tests don't work on miri
        insta::assert_debug_snapshot!("client_snapshot_test", encoded_output);

        let _ = encoded_output;
    }

    #[test]
    fn ignore_unknown_parameter() {
        use s2n_codec::EncoderBuffer;

        let value = client_transport_paramters();

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
}
