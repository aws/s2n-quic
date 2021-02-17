use crate::{
    ack,
    inet::SocketAddress,
    stream,
    transport::parameters::{
        AckDelayExponent, ActiveConnectionIdLimit, InitialFlowControlLimits, InitialMaxData,
        InitialMaxStreamDataBidiLocal, InitialMaxStreamDataBidiRemote, InitialMaxStreamDataUni,
        InitialMaxStreamsBidi, InitialMaxStreamsUni, InitialStreamLimits, MaxAckDelay,
        MaxIdleTimeout,
    },
};
use core::{convert::TryInto, time::Duration};

pub use crate::transport::parameters::ValidationError;

#[non_exhaustive]
#[derive(Debug)]
pub struct ConnectionInfo<'a> {
    pub source_adddress: &'a SocketAddress,
}

impl<'a> ConnectionInfo<'a> {
    pub fn new(source_adddress: &'a SocketAddress) -> Self {
        Self { source_adddress }
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
    pub(crate) max_open_unidirectional_streams: InitialMaxStreamsUni,
    pub(crate) max_ack_delay: MaxAckDelay,
    pub(crate) ack_delay_exponent: AckDelayExponent,
    pub(crate) max_active_connection_ids: ActiveConnectionIdLimit,
    pub(crate) ack_elicitation_interval: u8,
    pub(crate) ack_ranges_limit: u8,
    pub(crate) max_send_buffer_size: u32,
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
            max_open_unidirectional_streams: InitialMaxStreamsUni::RECOMMENDED,
            max_ack_delay: MaxAckDelay::RECOMMENDED,
            ack_delay_exponent: AckDelayExponent::RECOMMENDED,
            max_active_connection_ids: ActiveConnectionIdLimit::RECOMMENDED,
            ack_elicitation_interval: ack::Settings::RECOMMENDED.ack_elicitation_interval,
            ack_ranges_limit: ack::Settings::RECOMMENDED.ack_ranges_limit,
            max_send_buffer_size: stream::Limits::RECOMMENDED.max_send_buffer_size,
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
        with_max_open_unidirectional_streams,
        max_open_unidirectional_streams,
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

    pub const fn ack_settings(&self) -> ack::Settings {
        ack::Settings {
            ack_delay_exponent: self.ack_delay_exponent.as_u8(),
            max_ack_delay: self.max_ack_delay.as_duration(),
            ack_ranges_limit: self.ack_ranges_limit,
            ack_elicitation_interval: self.ack_elicitation_interval,
        }
    }

    pub const fn initial_flow_control_limits(&self) -> InitialFlowControlLimits {
        InitialFlowControlLimits {
            stream_limits: self.initial_stream_limits(),
            max_data: self.data_window.as_varint(),
            max_streams_bidi: self.max_open_bidirectional_streams.as_varint(),
            max_streams_uni: self.max_open_unidirectional_streams.as_varint(),
        }
    }

    pub const fn initial_stream_limits(&self) -> InitialStreamLimits {
        InitialStreamLimits {
            max_data_bidi_local: self.bidirectional_local_data_window.as_varint(),
            max_data_bidi_remote: self.bidirectional_remote_data_window.as_varint(),
            max_data_uni: self.unidirectional_data_window.as_varint(),
        }
    }

    pub const fn stream_limits(&self) -> stream::Limits {
        stream::Limits {
            max_send_buffer_size: self.max_send_buffer_size,
        }
    }
}

/// Creates limits for a given connection
pub trait Limiter {
    fn on_connection(&mut self, info: &ConnectionInfo) -> Limits;
}

/// Implement Limiter for a Limits struct
impl Limiter for Limits {
    fn on_connection(&mut self, _into: &ConnectionInfo) -> Limits {
        *self
    }
}
