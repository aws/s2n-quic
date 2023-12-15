// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack,
    event::{api::SocketAddress, IntoEvent},
    inet, recovery, stream,
    transport::parameters::{
        AckDelayExponent, ActiveConnectionIdLimit, InitialFlowControlLimits, InitialMaxData,
        InitialMaxStreamDataBidiLocal, InitialMaxStreamDataBidiRemote, InitialMaxStreamDataUni,
        InitialMaxStreamsBidi, InitialMaxStreamsUni, InitialStreamLimits, MaxAckDelay,
        MaxDatagramFrameSize, MaxIdleTimeout, TransportParameters,
    },
};
use core::{convert::TryInto, time::Duration};
use s2n_codec::decoder_invariant;

pub use crate::transport::parameters::ValidationError;

const MAX_HANDSHAKE_DURATION_DEFAULT: Duration = Duration::from_secs(10);

//= https://www.rfc-editor.org/rfc/rfc9000#section-10.1.2
//# A connection will time out if no packets are sent or received for a
//# period longer than the time negotiated using the max_idle_timeout
//# transport parameter; see Section 10.  However, state in middleboxes
//# might time out earlier than that.  Though REQ-5 in [RFC4787]
//# recommends a 2-minute timeout interval, experience shows that sending
//# packets every 30 seconds is necessary to prevent the majority of
//# middleboxes from losing state for UDP flows [GATEWAY].
const MAX_KEEP_ALIVE_PERIOD_DEFAULT: Duration = Duration::from_secs(30);

#[non_exhaustive]
#[derive(Debug)]
pub struct ConnectionInfo<'a> {
    pub remote_address: SocketAddress<'a>,
}

impl<'a> ConnectionInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(remote_address: &'a inet::SocketAddress) -> Self {
        Self {
            remote_address: remote_address.into_event(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub(crate) max_idle_timeout: MaxIdleTimeout,
    pub(crate) data_window: InitialMaxData,
    pub(crate) bidirectional_local_data_window: InitialMaxStreamDataBidiLocal,
    pub(crate) bidirectional_remote_data_window: InitialMaxStreamDataBidiRemote,
    pub(crate) unidirectional_data_window: InitialMaxStreamDataUni,
    pub(crate) max_open_local_bidirectional_streams: stream::limits::LocalBidirectional,
    pub(crate) max_open_local_unidirectional_streams: stream::limits::LocalUnidirectional,
    pub(crate) max_open_remote_bidirectional_streams: InitialMaxStreamsBidi,
    pub(crate) max_open_remote_unidirectional_streams: InitialMaxStreamsUni,
    pub(crate) max_ack_delay: MaxAckDelay,
    pub(crate) ack_delay_exponent: AckDelayExponent,
    pub(crate) max_active_connection_ids: ActiveConnectionIdLimit,
    pub(crate) ack_elicitation_interval: u8,
    pub(crate) ack_ranges_limit: u8,
    pub(crate) max_send_buffer_size: stream::limits::MaxSendBufferSize,
    pub(crate) max_handshake_duration: Duration,
    pub(crate) max_keep_alive_period: Duration,
    pub(crate) max_datagram_frame_size: MaxDatagramFrameSize,
    pub(crate) initial_round_trip_time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! setter {
    ($(#[doc = $doc:literal])* $name:ident, $field:ident, $inner:ty $(, |$validate_value:ident| $validation:block)?) => {
        $(#[doc = $doc])*
        pub fn $name(mut self, value: $inner) -> Result<Self, ValidationError> {
            $(
                let $validate_value = value;
                $validation
            )?
            self.$field = value.try_into()?;
            Ok(self)
        }
    };
}

impl Limits {
    pub const fn new() -> Self {
        Self {
            max_idle_timeout: MaxIdleTimeout::RECOMMENDED,
            data_window: InitialMaxData::RECOMMENDED,
            bidirectional_local_data_window: InitialMaxStreamDataBidiLocal::RECOMMENDED,
            bidirectional_remote_data_window: InitialMaxStreamDataBidiRemote::RECOMMENDED,
            unidirectional_data_window: InitialMaxStreamDataUni::RECOMMENDED,
            max_open_local_bidirectional_streams: stream::limits::LocalBidirectional::RECOMMENDED,
            max_open_local_unidirectional_streams: stream::limits::LocalUnidirectional::RECOMMENDED,
            max_open_remote_bidirectional_streams: InitialMaxStreamsBidi::RECOMMENDED,
            max_open_remote_unidirectional_streams: InitialMaxStreamsUni::RECOMMENDED,
            max_ack_delay: MaxAckDelay::RECOMMENDED,
            ack_delay_exponent: AckDelayExponent::RECOMMENDED,
            max_active_connection_ids: ActiveConnectionIdLimit::RECOMMENDED,
            ack_elicitation_interval: ack::Settings::RECOMMENDED.ack_elicitation_interval,
            ack_ranges_limit: ack::Settings::RECOMMENDED.ack_ranges_limit,
            max_send_buffer_size: stream::Limits::RECOMMENDED.max_send_buffer_size,
            max_handshake_duration: MAX_HANDSHAKE_DURATION_DEFAULT,
            max_keep_alive_period: MAX_KEEP_ALIVE_PERIOD_DEFAULT,
            max_datagram_frame_size: MaxDatagramFrameSize::DEFAULT,
            initial_round_trip_time: recovery::DEFAULT_INITIAL_RTT,
        }
    }

    // We limit the initial data limit to u32::MAX (4GB), which far
    // exceeds the reasonable amount of data a connection is
    // initially allowed to send.
    //
    // By representing the flow control value as a u32, we save space
    // on the connection state.
    setter!(with_data_window, data_window, u64, |validate_value| {
        decoder_invariant!(
            validate_value <= u32::MAX.into(),
            "data_window must be <= u32::MAX"
        );
    });
    setter!(
        with_bidirectional_local_data_window,
        bidirectional_local_data_window,
        u64,
        |validate_value| {
            decoder_invariant!(
                validate_value <= u32::MAX.into(),
                "bidirectional_local_data_window must be <= u32::MAX"
            );
        }
    );
    setter!(
        with_bidirectional_remote_data_window,
        bidirectional_remote_data_window,
        u64,
        |validate_value| {
            decoder_invariant!(
                validate_value <= u32::MAX.into(),
                "bidirectional_remote_data_window must be <= u32::MAX"
            );
        }
    );
    setter!(
        with_unidirectional_data_window,
        unidirectional_data_window,
        u64,
        |validate_value| {
            decoder_invariant!(
                validate_value <= u32::MAX.into(),
                "unidirectional_data_window must be <= u32::MAX"
            );
        }
    );

    setter!(with_max_idle_timeout, max_idle_timeout, Duration);

    /// Sets both the max local and remote limits for bidirectional streams.
    #[deprecated(
        note = "use with_max_open_local_bidirectional_streams and with_max_open_remote_bidirectional_streams instead"
    )]
    pub fn with_max_open_bidirectional_streams(
        mut self,
        value: u64,
    ) -> Result<Self, ValidationError> {
        self.max_open_local_bidirectional_streams = value.try_into()?;
        self.max_open_remote_bidirectional_streams = value.try_into()?;
        Ok(self)
    }

    /// Sets the max local limits for bidirectional streams
    ///
    /// The value set is used instead of `with_max_open_bidirectional_streams` when set.
    pub fn with_max_open_local_bidirectional_streams(
        mut self,
        value: u64,
    ) -> Result<Self, ValidationError> {
        self.max_open_local_bidirectional_streams = value.try_into()?;
        Ok(self)
    }

    /// Sets the max remote limits for bidirectional streams.
    ///
    /// The value set is used instead of `with_max_open_bidirectional_streams` when set.
    pub fn with_max_open_remote_bidirectional_streams(
        mut self,
        value: u64,
    ) -> Result<Self, ValidationError> {
        self.max_open_remote_bidirectional_streams = value.try_into()?;
        Ok(self)
    }

    setter!(
        with_max_open_local_unidirectional_streams,
        max_open_local_unidirectional_streams,
        u64
    );
    setter!(
        with_max_open_remote_unidirectional_streams,
        max_open_remote_unidirectional_streams,
        u64
    );
    setter!(with_max_ack_delay, max_ack_delay, Duration);
    setter!(
        with_max_active_connection_ids,
        max_active_connection_ids,
        u64
    );
    setter!(with_ack_elicitation_interval, ack_elicitation_interval, u8);
    setter!(with_max_ack_ranges, ack_ranges_limit, u8);
    setter!(
        /// Sets the maximum send buffer size for a Stream
        ///
        /// The send buffer contains unacknowledged application data. Constraining the maximum
        /// size of this buffer limits the amount of memory a given Stream may consume. On
        /// high bandwidth/high RTT connections this may act as a bottleneck, as the connection may be
        /// waiting for data to be acknowledged by the peer before allowing more data to be sent.
        /// Increasing this value should be carefully weighed against the potential downsides
        /// of additional memory utilization as well as increased latency due to the capacity of the
        /// send buffer exceeding the rate at which the network link and peer are able to drain from it.
        /// Ideally, the max_send_buffer_size is configured to the minimum value that can support the
        /// throughput requirements for the connection.
        with_max_send_buffer_size,
        max_send_buffer_size,
        u32
    );
    setter!(
        with_max_handshake_duration,
        max_handshake_duration,
        Duration
    );
    setter!(with_max_keep_alive_period, max_keep_alive_period, Duration);

    /// Sets the initial round trip time (RTT) for use in recovery mechanisms prior to
    /// measuring an actual RTT sample.
    ///
    /// This is useful for environments where RTTs are mostly predictable (e.g. data centers)
    /// and are much lower than the default 333 milliseconds.
    pub fn with_initial_round_trip_time(
        mut self,
        value: Duration,
    ) -> Result<Self, ValidationError> {
        ensure!(
            value >= recovery::MIN_RTT,
            Err(ValidationError(
                "provided value must be at least 1 microsecond",
            ))
        );

        self.initial_round_trip_time = value;
        Ok(self)
    }

    // internal APIs

    #[doc(hidden)]
    #[inline]
    pub fn load_peer<A, B, C, D>(&mut self, peer_parameters: &TransportParameters<A, B, C, D>) {
        self.max_idle_timeout
            .load_peer(&peer_parameters.max_idle_timeout);
    }

    #[doc(hidden)]
    #[inline]
    pub const fn ack_settings(&self) -> ack::Settings {
        ack::Settings {
            ack_delay_exponent: self.ack_delay_exponent.as_u8(),
            max_ack_delay: self.max_ack_delay.as_duration(),
            ack_ranges_limit: self.ack_ranges_limit,
            ack_elicitation_interval: self.ack_elicitation_interval,
        }
    }

    #[doc(hidden)]
    #[inline]
    pub const fn initial_flow_control_limits(&self) -> InitialFlowControlLimits {
        InitialFlowControlLimits {
            stream_limits: self.initial_stream_limits(),
            max_data: self.data_window.as_varint(),
            max_open_remote_bidirectional_streams: self
                .max_open_remote_bidirectional_streams
                .as_varint(),
            max_open_remote_unidirectional_streams: self
                .max_open_remote_unidirectional_streams
                .as_varint(),
        }
    }

    #[doc(hidden)]
    #[inline]
    pub const fn initial_stream_limits(&self) -> InitialStreamLimits {
        InitialStreamLimits {
            max_data_bidi_local: self.bidirectional_local_data_window.as_varint(),
            max_data_bidi_remote: self.bidirectional_remote_data_window.as_varint(),
            max_data_uni: self.unidirectional_data_window.as_varint(),
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn stream_limits(&self) -> stream::Limits {
        stream::Limits {
            max_send_buffer_size: self.max_send_buffer_size,
            max_open_local_unidirectional_streams: self.max_open_local_unidirectional_streams,
            max_open_local_bidirectional_streams: self.max_open_local_bidirectional_streams,
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn max_idle_timeout(&self) -> Option<Duration> {
        self.max_idle_timeout.as_duration()
    }

    #[doc(hidden)]
    #[inline]
    pub fn max_handshake_duration(&self) -> Duration {
        self.max_handshake_duration
    }

    #[doc(hidden)]
    #[inline]
    pub fn max_keep_alive_period(&self) -> Duration {
        self.max_keep_alive_period
    }

    #[doc(hidden)]
    #[inline]
    pub fn initial_round_trip_time(&self) -> Duration {
        self.initial_round_trip_time
    }
}

/// Creates limits for a given connection
pub trait Limiter: 'static + Send {
    fn on_connection(&mut self, info: &ConnectionInfo) -> Limits;
}

/// Implement Limiter for a Limits struct
impl Limiter for Limits {
    fn on_connection(&mut self, _into: &ConnectionInfo) -> Limits {
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Local max data limits should be <= u32::MAX
    #[test]
    fn limit_validation() {
        let mut data = u32::MAX as u64 + 1;
        let limits = Limits::default();
        assert!(limits.with_data_window(data).is_err());
        assert!(limits.with_bidirectional_local_data_window(data).is_err());
        assert!(limits.with_bidirectional_remote_data_window(data).is_err());
        assert!(limits.with_unidirectional_data_window(data).is_err());

        data = u32::MAX as u64;
        assert!(limits.with_data_window(data).is_ok());
        assert!(limits.with_bidirectional_local_data_window(data).is_ok());
        assert!(limits.with_bidirectional_remote_data_window(data).is_ok());
        assert!(limits.with_unidirectional_data_window(data).is_ok());
    }
}
