// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack,
    event::{api::SocketAddress, IntoEvent},
    inet, stream,
    stream::limits::{LocalMaxSendBufferSize, LocalUniDirectionalLimit},
    transport::parameters::{
        AckDelayExponent, ActiveConnectionIdLimit, InitialFlowControlLimits, InitialMaxData,
        InitialMaxStreamDataBidiLocal, InitialMaxStreamDataBidiRemote, InitialMaxStreamDataUni,
        InitialMaxStreamsBidi, InitialMaxStreamsUni, InitialStreamLimits, MaxAckDelay,
        MaxDatagramFrameSize, MaxIdleTimeout, TransportParameters,
    },
};
use core::{convert::TryInto, time::Duration};

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
    pub(crate) max_open_bidirectional_streams: InitialMaxStreamsBidi,
    pub(crate) max_open_local_unidirectional_streams: LocalUniDirectionalLimit,
    pub(crate) max_open_remote_unidirectional_streams: InitialMaxStreamsUni,
    pub(crate) max_ack_delay: MaxAckDelay,
    pub(crate) ack_delay_exponent: AckDelayExponent,
    pub(crate) max_active_connection_ids: ActiveConnectionIdLimit,
    pub(crate) ack_elicitation_interval: u8,
    pub(crate) ack_ranges_limit: u8,
    pub(crate) max_send_buffer_size: LocalMaxSendBufferSize,
    pub(crate) max_handshake_duration: Duration,
    pub(crate) max_keep_alive_period: Duration,
    pub(crate) max_datagram_frame_size: MaxDatagramFrameSize,
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! setter {
    ($name:ident, $field:ident, $inner:ty) => {
        pub fn $name(mut self, value: $inner) -> Result<Self, ValidationError> {
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
            max_open_bidirectional_streams: InitialMaxStreamsBidi::RECOMMENDED,
            max_open_local_unidirectional_streams: LocalUniDirectionalLimit::RECOMMENDED,
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
        }
    }

    setter!(with_max_idle_timeout, max_idle_timeout, Duration);
    setter!(with_data_window, data_window, u64);
    setter!(
        with_bidirectional_local_data_window,
        bidirectional_local_data_window,
        u64
    );
    setter!(
        with_bidirectional_remote_data_window,
        bidirectional_remote_data_window,
        u64
    );
    setter!(
        with_unidirectional_data_window,
        unidirectional_data_window,
        u64
    );
    setter!(
        with_max_open_bidirectional_streams,
        max_open_bidirectional_streams,
        u64
    );
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
    setter!(with_max_send_buffer_size, max_send_buffer_size, u32);
    setter!(
        with_max_handshake_duration,
        max_handshake_duration,
        Duration
    );
    setter!(with_max_keep_alive_period, max_keep_alive_period, Duration);

    // internal APIs

    #[doc(hidden)]
    pub fn load_peer<A, B, C, D>(&mut self, peer_parameters: &TransportParameters<A, B, C, D>) {
        self.max_idle_timeout
            .load_peer(&peer_parameters.max_idle_timeout);
    }

    #[doc(hidden)]
    pub const fn ack_settings(&self) -> ack::Settings {
        ack::Settings {
            ack_delay_exponent: self.ack_delay_exponent.as_u8(),
            max_ack_delay: self.max_ack_delay.as_duration(),
            ack_ranges_limit: self.ack_ranges_limit,
            ack_elicitation_interval: self.ack_elicitation_interval,
        }
    }

    #[doc(hidden)]
    pub const fn initial_flow_control_limits(&self) -> InitialFlowControlLimits {
        InitialFlowControlLimits {
            stream_limits: self.initial_stream_limits(),
            max_data: self.data_window.as_varint(),
            max_streams_bidi: self.max_open_bidirectional_streams.as_varint(),
            max_streams_uni: self.max_open_remote_unidirectional_streams.as_varint(),
        }
    }

    #[doc(hidden)]
    pub const fn initial_stream_limits(&self) -> InitialStreamLimits {
        InitialStreamLimits {
            max_data_bidi_local: self.bidirectional_local_data_window.as_varint(),
            max_data_bidi_remote: self.bidirectional_remote_data_window.as_varint(),
            max_data_uni: self.unidirectional_data_window.as_varint(),
        }
    }

    #[doc(hidden)]
    pub fn stream_limits(&self) -> stream::Limits {
        stream::Limits {
            max_send_buffer_size: self.max_send_buffer_size,
            max_open_local_unidirectional_streams: self.max_open_local_unidirectional_streams,
            max_open_local_bidirectional_streams: self.max_open_bidirectional_streams.into(),
        }
    }

    #[doc(hidden)]
    pub fn max_idle_timeout(&self) -> Option<Duration> {
        self.max_idle_timeout.as_duration()
    }

    #[doc(hidden)]
    pub fn max_handshake_duration(&self) -> Duration {
        self.max_handshake_duration
    }

    #[doc(hidden)]
    pub fn max_keep_alive_period(&self) -> Duration {
        self.max_keep_alive_period
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
