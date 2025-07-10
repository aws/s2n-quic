// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    event::{self, builder::MtuUpdatedCause, IntoEvent},
    frame, inet,
    packet::number::PacketNumber,
    path,
    path::mtu,
    recovery::{congestion_controller, CongestionController},
    time::{timer, Timer, Timestamp},
    transmission,
};
use core::{
    fmt,
    fmt::{Display, Formatter},
    num::NonZeroU16,
    time::Duration,
};
use s2n_codec::EncoderValue;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::inet::{IpV4Address, SocketAddressV4};

    /// Creates a new mtu::Controller with an IPv4 address and the given `max_mtu`
    pub fn new_controller(max_mtu: u16) -> Controller {
        let ip = IpV4Address::new([127, 0, 0, 1]);
        let addr = inet::SocketAddress::IpV4(SocketAddressV4::new(ip, 443));
        Controller::new(
            Config {
                max_mtu: max_mtu.try_into().unwrap(),
                ..Default::default()
            },
            &addr,
        )
    }

    /// Creates a new mtu::Controller with the given mtu and probed size
    pub fn test_controller(mtu: u16, probed_size: u16) -> Controller {
        let mut controller = new_controller(u16::MAX);
        controller.plpmtu = mtu;
        controller.probed_size = probed_size;
        controller
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    /// EARLY_SEARCH_REQUESTED indicates the initial MTU was configured higher
    /// than the base MTU, to allow for quick confirmation or rejection of the
    /// initial MTU
    EarlySearchRequested,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The DISABLED state is the initial state before probing has started.
    Disabled,
    /// SEARCH_REQUESTED is used to indicate a probe packet has been requested
    /// to be transmitted, but has not been transmitted yet.
    SearchRequested,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The SEARCHING state is the main probing state.
    Searching(PacketNumber, Timestamp),
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The SEARCH_COMPLETE state indicates that a search has completed.
    SearchComplete,
}

impl State {
    /// Returns true if the MTU controller is in the early search requested state
    fn is_early_search_requested(&self) -> bool {
        matches!(self, State::EarlySearchRequested)
    }

    /// Returns true if the MTU controller is in the disabled state
    fn is_disabled(&self) -> bool {
        matches!(self, State::Disabled)
    }

    /// Returns true if the MTU controller is in the search complete state
    fn is_search_complete(&self) -> bool {
        matches!(self, State::SearchComplete)
    }
}

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
//# The MAX_PROBES is the maximum value of the PROBE_COUNT
//# counter (see Section 5.1.3).  MAX_PROBES represents the limit for
//# the number of consecutive probe attempts of any size.  Search
//# algorithms benefit from a MAX_PROBES value greater than 1 because
//# this can provide robustness to isolated packet loss.  The default
//# value of MAX_PROBES is 3.
const MAX_PROBES: u8 = 3;

/// The minimum length of the data field of a packet sent over an
/// Ethernet is 1500 octets, thus the maximum length of an IP datagram
/// sent over an Ethernet is 1500 octets.
/// See https://www.rfc-editor.org/rfc/rfc894.txt
const ETHERNET_MTU: u16 = 1500;

/// If the next value to probe is within the PROBE_THRESHOLD bytes of
/// the current Path MTU, probing will be considered complete.
const PROBE_THRESHOLD: u16 = 20;

/// When the black_hole_counter exceeds this threshold, on_black_hole_detected will be
/// called to reduce the MTU to the BASE_PLPMTU. The black_hole_counter is incremented when
/// a burst of consecutive packets is lost that starts with a packet that is:
///      1) not an MTU probe
///      2) larger than the BASE_PLPMTU
///      3) sent after the largest MTU-sized acknowledged packet number
/// This is a possible indication that the path cannot support the MTU that was previously confirmed.
const BLACK_HOLE_THRESHOLD: u8 = 3;

/// After a black hole has been detected, the mtu::Controller will wait this duration
/// before probing for a larger MTU again.
const BLACK_HOLE_COOL_OFF_DURATION: Duration = Duration::from_secs(60);

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.1
//# The PMTU_RAISE_TIMER is configured to the period a
//# sender will continue to use the current PLPMTU, after which it
//# reenters the Search Phase.  This timer has a period of 600
//# seconds, as recommended by PLPMTUD [RFC4821].
const PMTU_RAISE_TIMER_DURATION: Duration = Duration::from_secs(600);

//= https://www.rfc-editor.org/rfc/rfc9000#section-14
//# QUIC MUST NOT be used if the network path cannot support a
//# maximum datagram size of at least 1200 bytes.

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
//# When using IPv4, there is no currently equivalent size specified,
//# and a default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
pub const MINIMUM_MAX_DATAGRAM_SIZE: u16 = 1200;

// Length is the length in octets of this user datagram  including  this
// header and the data. (This means the minimum value of the length is
// eight.)
// See https://www.rfc-editor.org/rfc/rfc768.txt
const UDP_HEADER_LEN: u16 = 8;

// IPv4 header ranges from 20-60 bytes, depending on Options
const IPV4_MIN_HEADER_LEN: u16 = 20;
// IPv6 header is always 40 bytes, plus extensions
const IPV6_MIN_HEADER_LEN: u16 = 40;

// The minimum allowed Max MTU is the minimum UDP datagram size of 1200 bytes plus
// the UDP header length and minimal IP header length
const fn const_min(a: u16, b: u16) -> u16 {
    if a < b {
        a
    } else {
        b
    }
}

const MINIMUM_MTU: u16 = MINIMUM_MAX_DATAGRAM_SIZE
    + UDP_HEADER_LEN
    + const_min(IPV4_MIN_HEADER_LEN, IPV6_MIN_HEADER_LEN);

macro_rules! impl_mtu {
    ($name:ident, $default:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name(NonZeroU16);

        impl $name {
            /// The minimum value required for path MTU
            pub const MIN: Self = Self(unsafe { NonZeroU16::new_unchecked(MINIMUM_MTU) });

            /// The largest size of a QUIC datagram that can be sent on a path that supports this
            /// MTU. This does not include the size of UDP and IP headers.
            #[inline]
            pub fn max_datagram_size(&self, peer_socket_address: &inet::SocketAddress) -> u16 {
                let min_ip_header_len = match peer_socket_address {
                    inet::SocketAddress::IpV4(_) => IPV4_MIN_HEADER_LEN,
                    inet::SocketAddress::IpV6(_) => IPV6_MIN_HEADER_LEN,
                };
                (u16::from(*self) - UDP_HEADER_LEN - min_ip_header_len)
                    .max(MINIMUM_MAX_DATAGRAM_SIZE)
            }
        }

        impl Default for $name {
            #[inline]
            fn default() -> Self {
                $default
            }
        }

        impl TryFrom<u16> for $name {
            type Error = MtuError;

            fn try_from(value: u16) -> Result<Self, Self::Error> {
                if value < MINIMUM_MTU {
                    return Err(MtuError);
                }

                Ok($name(value.try_into().expect(
                    "Value must be greater than zero according to the check above",
                )))
            }
        }

        impl From<$name> for usize {
            #[inline]
            fn from(value: $name) -> Self {
                value.0.get() as usize
            }
        }

        impl From<$name> for u16 {
            #[inline]
            fn from(value: $name) -> Self {
                value.0.get()
            }
        }
    };
}

// Safety: 1500 and MINIMUM_MTU are greater than zero
const DEFAULT_MAX_MTU: MaxMtu = MaxMtu(unsafe { NonZeroU16::new_unchecked(1500) });
const DEFAULT_BASE_MTU: BaseMtu = BaseMtu(unsafe { NonZeroU16::new_unchecked(MINIMUM_MTU) });
const DEFAULT_INITIAL_MTU: InitialMtu =
    InitialMtu(unsafe { NonZeroU16::new_unchecked(MINIMUM_MTU) });

impl_mtu!(MaxMtu, DEFAULT_MAX_MTU);
impl_mtu!(InitialMtu, DEFAULT_INITIAL_MTU);
impl_mtu!(BaseMtu, DEFAULT_BASE_MTU);

#[derive(Debug, Eq, PartialEq)]
pub struct MtuError;

impl Display for MtuError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MTU must have {} <= base_mtu (default: {}) <= initial_mtu (default: {}) <= max_mtu (default: {})",
            MINIMUM_MTU, DEFAULT_BASE_MTU.0, DEFAULT_INITIAL_MTU.0, DEFAULT_MAX_MTU.0
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for MtuError {}

/// Information about the path that may be used when generating MTU configuration.
#[non_exhaustive]
pub struct PathInfo<'a> {
    pub remote_address: event::api::SocketAddress<'a>,
}

impl<'a> PathInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(remote_address: &'a inet::SocketAddress) -> Self {
        PathInfo {
            remote_address: remote_address.into_event(),
        }
    }
}

/// MTU configuration manager.
#[derive(Debug)]
pub struct Manager<E: mtu::Endpoint> {
    provider: E,
    endpoint_mtu_config: Config,
}

impl<E: mtu::Endpoint> Manager<E> {
    pub fn new(provider: E) -> Self {
        Manager {
            provider,
            // Instantiate the Manager with default values since the endpoint is
            // created before the IO provider (IO provider sets the actual config `set_mtu_config()`).
            endpoint_mtu_config: Default::default(),
        }
    }

    pub fn config(&mut self, remote_address: &inet::SocketAddress) -> Result<Config, MtuError> {
        let info = mtu::PathInfo::new(remote_address);
        if let Some(conn_config) = self.provider.on_path(&info, self.endpoint_mtu_config) {
            ensure!(conn_config.is_valid(), Err(MtuError));
            ensure!(
                u16::from(conn_config.max_mtu) <= u16::from(self.endpoint_mtu_config.max_mtu()),
                Err(MtuError)
            );

            Ok(conn_config)
        } else {
            Ok(self.endpoint_mtu_config)
        }
    }

    pub fn set_endpoint_config(&mut self, config: Config) {
        self.endpoint_mtu_config = config;
    }

    pub fn endpoint_config(&self) -> &Config {
        &self.endpoint_mtu_config
    }
}

/// Specify MTU configuration for the given path.
pub trait Endpoint: 'static + Send {
    /// Provide path specific MTU config.
    ///
    /// The MTU provider is invoked for each new path established during a
    /// connection. Returning `None` means that the path should inherit
    /// the endpoint configured values.
    ///
    /// Application must ensure that `max_mtu <= endpoint_mtu_config.max_mtu()`.
    fn on_path(&mut self, info: &mtu::PathInfo, endpoint_mtu_config: Config)
        -> Option<mtu::Config>;
}

/// Inherit the endpoint configured values.
#[derive(Debug, Default)]
pub struct Inherit {}

impl Endpoint for Inherit {
    fn on_path(
        &mut self,
        _info: &mtu::PathInfo,
        _endpoint_mtu_config: Config,
    ) -> Option<mtu::Config> {
        None
    }
}

/// MTU configuration.
#[derive(Copy, Clone, Debug, Default)]
pub struct Config {
    initial_mtu: InitialMtu,
    base_mtu: BaseMtu,
    max_mtu: MaxMtu,
}

impl Endpoint for Config {
    fn on_path(
        &mut self,
        _info: &mtu::PathInfo,
        _endpoint_mtu_config: Config,
    ) -> Option<mtu::Config> {
        Some(*self)
    }
}

impl Config {
    pub const MIN: Self = Self {
        initial_mtu: InitialMtu::MIN,
        base_mtu: BaseMtu::MIN,
        max_mtu: MaxMtu::MIN,
    };

    pub fn builder() -> Builder {
        Builder::default()
    }

    /// The maximum transmission unit (MTU) to use when initiating a connection.
    pub fn initial_mtu(&self) -> InitialMtu {
        self.initial_mtu
    }

    /// The smallest maximum transmission unit (MTU) to use when transmitting.
    pub fn base_mtu(&self) -> BaseMtu {
        self.base_mtu
    }

    /// The largest maximum transmission unit (MTU) that can be sent on a path.
    pub fn max_mtu(&self) -> MaxMtu {
        self.max_mtu
    }

    /// Returns true if the MTU configuration is valid
    ///
    /// A valid MTU configuration must have base_mtu <= initial_mtu <= max_mtu
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.base_mtu.0 <= self.initial_mtu.0 && self.initial_mtu.0 <= self.max_mtu.0
    }
}

#[derive(Debug, Default)]
pub struct Builder {
    initial_mtu: Option<InitialMtu>,
    base_mtu: Option<BaseMtu>,
    max_mtu: Option<MaxMtu>,
}

impl Builder {
    /// Sets the maximum transmission unit (MTU) to use when initiating a connection (default: 1228)
    ///
    /// For a detailed description see the [with_initial_mtu] documentation in the IO provider.
    ///
    /// [with_initial_mtu]: https://docs.rs/s2n-quic/latest/s2n_quic/provider/io/tokio/struct.Builder.html#method.with_initial_mtu
    pub fn with_initial_mtu(mut self, initial_mtu: u16) -> Result<Self, MtuError> {
        if let Some(base_mtu) = self.base_mtu {
            ensure!(initial_mtu >= base_mtu.0.get(), Err(MtuError));
        }

        if let Some(max_mtu) = self.max_mtu {
            ensure!(initial_mtu <= max_mtu.0.get(), Err(MtuError));
        }

        self.initial_mtu = Some(initial_mtu.try_into()?);
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path (default: 1500)
    ///
    /// For a detailed description see the [with_base_mtu] documentation in the IO provider.
    ///
    /// [with_base_mtu]: https://docs.rs/s2n-quic/latest/s2n_quic/provider/io/tokio/struct.Builder.html#method.with_base_mtu
    pub fn with_base_mtu(mut self, base_mtu: u16) -> Result<Self, MtuError> {
        if let Some(initial_mtu) = self.initial_mtu {
            ensure!(initial_mtu.0.get() >= base_mtu, Err(MtuError));
        }

        if let Some(max_mtu) = self.max_mtu {
            ensure!(base_mtu <= max_mtu.0.get(), Err(MtuError));
        }

        self.base_mtu = Some(base_mtu.try_into()?);
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path (default: 1500)
    ///
    /// Application must ensure that max_mtu <= endpoint_mtu_config.max_mtu(). For a detailed
    /// description see the [with_max_mtu] documentation in the IO provider.
    ///
    /// [with_max_mtu]: https://docs.rs/s2n-quic/latest/s2n_quic/provider/io/tokio/struct.Builder.html#method.with_max_mtu
    pub fn with_max_mtu(mut self, max_mtu: u16) -> Result<Self, MtuError> {
        if let Some(initial_mtu) = self.initial_mtu {
            ensure!(initial_mtu.0.get() <= max_mtu, Err(MtuError));
        }

        if let Some(base_mtu) = self.base_mtu {
            ensure!(base_mtu.0.get() <= max_mtu, Err(MtuError));
        }

        self.max_mtu = Some(max_mtu.try_into()?);
        Ok(self)
    }

    pub fn build(self) -> Result<Config, MtuError> {
        let base_mtu = self.base_mtu.unwrap_or_default();
        let max_mtu = self.max_mtu.unwrap_or_default();
        let mut initial_mtu = self.initial_mtu.unwrap_or_default();

        if self.initial_mtu.is_none() {
            // The initial_mtu was not configured, so adjust the value from the default
            // based on the default or configured base and max MTUs
            initial_mtu = initial_mtu
                .0
                .max(base_mtu.0)
                .min(max_mtu.0)
                .get()
                .try_into()?
        };

        let config = Config {
            initial_mtu,
            max_mtu,
            base_mtu,
        };

        ensure!(config.is_valid(), Err(MtuError));
        Ok(config)
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum MtuResult {
    NoChange,
    MtuUpdated(u16),
}

#[derive(Clone, Debug)]
pub struct Controller {
    state: State,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
    //# The BASE_PLPMTU is a configured size expected to work for most paths.
    //# The size is equal to or larger than the MIN_PLPMTU and smaller than
    //# the MAX_PLPMTU.
    base_plpmtu: u16,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-2
    //# The Packetization Layer PMTU is an estimate of the largest size
    //# of PL datagram that can be sent by a path, controlled by PLPMTUD
    plpmtu: u16,
    /// The maximum size the UDP payload can reach for any probe packet.
    max_udp_payload: u16,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.3
    //# The PROBED_SIZE is the size of the current probe packet
    //# as determined at the PL.  This is a tentative value for the
    //# PLPMTU, which is awaiting confirmation by an acknowledgment.
    probed_size: u16,
    /// The maximum size datagram to probe for. In contrast to the max_udp_payload,
    /// this value will decrease if probes are not acknowledged.
    max_probe_size: u16,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.3
    //# The PROBE_COUNT is a count of the number of successive
    //# unsuccessful probe packets that have been sent.
    probe_count: u8,
    /// A count of the number of packets with a size > base_plpmtu lost since
    /// the last time a packet with size equal to the current MTU was acknowledged.
    black_hole_counter: Counter<u8, Saturating>,
    /// The largest acknowledged packet with size >= the plpmtu. Used when tracking
    /// packets that have been lost for the purpose of detecting a black hole.
    largest_acked_mtu_sized_packet: Option<PacketNumber>,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.1
    //# The PMTU_RAISE_TIMER is configured to the period a
    //# sender will continue to use the current PLPMTU, after which it
    //# reenters the Search Phase.
    pmtu_raise_timer: Timer,
}

impl Controller {
    /// Construct a new mtu::Controller with the given `max_mtu` and `peer_socket_address`
    ///
    /// The UDP header length and IP header length will be subtracted from `max_mtu` to
    /// determine the max_udp_payload used for limiting the payload length of probe packets.
    /// max_mtu is the maximum allowed mtu, e.g. for jumbo frames this value is expected to
    /// be over 9000.
    #[inline]
    pub fn new(config: Config, peer_socket_address: &inet::SocketAddress) -> Self {
        debug_assert!(config.is_valid(), "Invalid MTU configuration {config:?}");

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.3
        //# Endpoints SHOULD set the initial value of BASE_PLPMTU (Section 5.1 of
        //# [DPLPMTUD]) to be consistent with QUIC's smallest allowed maximum
        //# datagram size.
        let base_plpmtu = config.base_mtu.max_datagram_size(peer_socket_address);
        let max_udp_payload = config.max_mtu.max_datagram_size(peer_socket_address);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
        //# Datagrams containing Initial packets MAY exceed 1200 bytes if the sender
        //# believes that the network path and peer both support the size that it chooses.
        let plpmtu = config.initial_mtu.max_datagram_size(peer_socket_address);

        let initial_probed_size = if u16::from(config.initial_mtu) > ETHERNET_MTU - PROBE_THRESHOLD
        {
            // An initial MTU was provided within the probe threshold of the Ethernet MTU, so we can
            // instead try probing for an MTU larger than the Ethernet MTU
            Self::next_probe_size(plpmtu, max_udp_payload)
        } else {
            // The UDP payload size for the most likely MTU is based on standard Ethernet MTU minus
            // the minimum length IP headers (without IPv4 options or IPv6 extensions) and UPD header
            let min_ip_header_len = match peer_socket_address {
                inet::SocketAddress::IpV4(_) => IPV4_MIN_HEADER_LEN,
                inet::SocketAddress::IpV6(_) => IPV6_MIN_HEADER_LEN,
            };
            ETHERNET_MTU - UDP_HEADER_LEN - min_ip_header_len
        }
        .min(max_udp_payload);

        let state = if plpmtu > base_plpmtu {
            // The initial MTU has been configured higher than the base MTU
            State::EarlySearchRequested
        } else if initial_probed_size - base_plpmtu < PROBE_THRESHOLD {
            // The next probe size is within the probe threshold of the
            // base MTU, so no probing will occur and the search is complete
            State::SearchComplete
        } else {
            // Otherwise wait for regular MTU probing to be enabled
            State::Disabled
        };

        Self {
            state,
            base_plpmtu,
            plpmtu,
            probed_size: initial_probed_size,
            max_udp_payload,
            max_probe_size: max_udp_payload,
            probe_count: 0,
            black_hole_counter: Default::default(),
            largest_acked_mtu_sized_packet: None,
            pmtu_raise_timer: Timer::default(),
        }
    }

    /// Enable path MTU probing
    #[inline]
    pub fn enable(&mut self) {
        // ensure we haven't already enabled the controller
        ensure!(self.state.is_disabled() || self.state.is_early_search_requested());

        // TODO: Look up current MTU in a cache. If there is a cache hit
        //       move directly to SearchComplete and arm the PMTU raise timer.
        //       Otherwise, start searching for a larger PMTU immediately
        self.request_new_search(None);
    }

    /// Called when the connection timer expires
    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        ensure!(self.pmtu_raise_timer.poll_expiration(now).is_ready());
        self.request_new_search(None);
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.2
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    /// This method gets called when a packet delivery got acknowledged
    #[inline]
    pub fn on_packet_ack<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        sent_bytes: u16,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> MtuResult {
        if self.state.is_early_search_requested() && sent_bytes > self.base_plpmtu {
            if self.is_next_probe_size_above_threshold() {
                // Early probing has succeeded, but the max MTU is higher still so
                // wait for regular MTU probing to be enabled to attempt higher MTUs
                self.state = State::Disabled;
            } else {
                self.state = State::SearchComplete;
            }

            // Publish an `on_mtu_updated` event since the cause
            // and possibly search_complete status have changed
            publisher.on_mtu_updated(event::builder::MtuUpdated {
                path_id: path_id.into_event(),
                mtu: self.plpmtu,
                cause: MtuUpdatedCause::InitialMtuPacketAcknowledged,
                search_complete: self.state.is_search_complete(),
            });
        }

        // no need to process anything in the disabled state
        ensure!(self.state != State::Disabled, MtuResult::NoChange);

        // MTU probes are only sent in application data space
        ensure!(
            packet_number.space().is_application_data(),
            MtuResult::NoChange
        );

        if sent_bytes >= self.plpmtu
            && self
                .largest_acked_mtu_sized_packet
                .is_none_or(|pn| packet_number > pn)
        {
            // Reset the black hole counter since a packet the size of the current MTU or larger
            // has been acknowledged, indicating the path can still support the current MTU
            self.black_hole_counter = Default::default();
            self.largest_acked_mtu_sized_packet = Some(packet_number);
        }

        if let State::Searching(probe_packet_number, transmit_time) = self.state {
            if packet_number == probe_packet_number {
                self.plpmtu = self.probed_size;
                // A new MTU has been confirmed, notify the congestion controller
                congestion_controller.on_mtu_update(
                    self.plpmtu,
                    &mut congestion_controller::PathPublisher::new(publisher, path_id),
                );

                self.update_probed_size();

                //= https://www.rfc-editor.org/rfc/rfc8899#section-8
                //# To avoid excessive load, the interval between individual probe
                //# packets MUST be at least one RTT, and the interval between rounds of
                //# probing is determined by the PMTU_RAISE_TIMER.

                // Subsequent probe packets are sent based on the round trip transmission and
                // acknowledgement/loss of a packet, so the interval will be at least 1 RTT.
                self.request_new_search(Some(transmit_time));

                publisher.on_mtu_updated(event::builder::MtuUpdated {
                    path_id: path_id.into_event(),
                    mtu: self.plpmtu,
                    cause: MtuUpdatedCause::ProbeAcknowledged,
                    search_complete: self.state.is_search_complete(),
                });

                return MtuResult::MtuUpdated(self.plpmtu);
            }
        }

        MtuResult::NoChange
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# The PL is REQUIRED to be
    //# robust in the case where probe packets are lost due to other
    //# reasons (including link transmission error, congestion).
    /// This method gets called when a packet loss is reported
    #[inline]
    pub fn on_packet_loss<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        lost_bytes: u16,
        new_loss_burst: bool,
        now: Timestamp,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> MtuResult {
        // MTU probes are only sent in the application data space, but since early packet
        // spaces will use the `InitialMtu` prior to MTU probing being enabled, we need
        // to check for potentially MTU-related packet loss if an early search has been requested
        ensure!(
            self.state.is_early_search_requested() || packet_number.space().is_application_data(),
            MtuResult::NoChange
        );

        match &self.state {
            State::Disabled => {}
            State::EarlySearchRequested => {
                // MTU probing hasn't been enabled yet, but since the initial MTU was configured
                // higher than the base PLPMTU and this setting resulted in a lost packet
                // we drop back down to the base PLPMTU.
                self.plpmtu = self.base_plpmtu;

                congestion_controller.on_mtu_update(
                    self.plpmtu,
                    &mut congestion_controller::PathPublisher::new(publisher, path_id),
                );

                if self.is_next_probe_size_above_threshold() {
                    // Resume regular probing when the MTU controller is enabled
                    self.state = State::Disabled;
                } else {
                    // The next probe is within the threshold, so move directly
                    // to the SearchComplete state
                    self.state = State::SearchComplete;
                }

                publisher.on_mtu_updated(event::builder::MtuUpdated {
                    path_id: path_id.into_event(),
                    mtu: self.plpmtu,
                    cause: MtuUpdatedCause::InitialMtuPacketLost,
                    search_complete: self.state.is_search_complete(),
                });

                return MtuResult::MtuUpdated(self.plpmtu);
            }
            State::Searching(probe_pn, _) if *probe_pn == packet_number => {
                // The MTU probe was lost
                if self.probe_count == MAX_PROBES {
                    // We've sent MAX_PROBES without acknowledgement, so
                    // attempt a smaller probe size
                    self.max_probe_size = self.probed_size;
                    self.update_probed_size();
                    self.request_new_search(None);

                    if self.is_search_completed() {
                        // Emit an on_mtu_updated event as the search has now completed
                        publisher.on_mtu_updated(event::builder::MtuUpdated {
                            path_id: path_id.into_event(),
                            mtu: self.plpmtu,
                            cause: MtuUpdatedCause::LargerProbesLost,
                            search_complete: true,
                        })
                    }
                } else {
                    // Try the same probe size again
                    self.state = State::SearchRequested
                }
            }
            State::Searching(_, _) | State::SearchComplete | State::SearchRequested => {
                if (self.base_plpmtu + 1..=self.plpmtu).contains(&lost_bytes)
                    && self
                        .largest_acked_mtu_sized_packet
                        .is_none_or(|pn| packet_number > pn)
                    && new_loss_burst
                {
                    // A non-probe packet larger than the BASE_PLPMTU that was sent after the last
                    // acknowledged MTU-sized packet has been lost
                    self.black_hole_counter += 1;
                }

                if self.black_hole_counter > BLACK_HOLE_THRESHOLD {
                    return self.on_black_hole_detected(
                        now,
                        congestion_controller,
                        path_id,
                        publisher,
                    );
                }
            }
        }

        MtuResult::NoChange
    }

    /// Gets the currently validated maximum QUIC datagram size
    ///
    /// This does not include the size of UDP and IP headers.
    #[inline]
    pub fn max_datagram_size(&self) -> usize {
        self.plpmtu as usize
    }

    /// Gets the max datagram size currently being probed for
    #[inline]
    pub fn probed_sized(&self) -> usize {
        self.probed_size as usize
    }

    /// Returns true if probing for the MTU has completed
    pub fn is_search_completed(&self) -> bool {
        self.state.is_search_complete()
    }

    /// Sets `probed_size` to the next MTU size to probe for based on a binary search
    #[inline]
    fn update_probed_size(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.3.2
        //# Implementations SHOULD select the set of probe packet sizes to
        //# maximize the gain in PLPMTU from each search step.
        self.probed_size = Self::next_probe_size(self.plpmtu, self.max_probe_size);
    }

    /// Calculates the next probe size as halfway from the current to the max size
    #[inline]
    fn next_probe_size(current: u16, max: u16) -> u16 {
        current + ((max - current) / 2)
    }

    #[inline]
    fn is_next_probe_size_above_threshold(&self) -> bool {
        self.probed_size - self.plpmtu >= PROBE_THRESHOLD
    }

    /// Requests a new search to be initiated
    ///
    /// If `last_probe_time` is supplied, the PMTU Raise Timer will be armed as
    /// necessary if the probed_size is already within the PROBE_THRESHOLD
    /// of the current PLPMTU
    #[inline]
    fn request_new_search(&mut self, last_probe_time: Option<Timestamp>) {
        if self.is_next_probe_size_above_threshold() {
            self.probe_count = 0;
            self.state = State::SearchRequested;
        } else {
            // The next probe size is within the threshold of the current MTU
            // so its not worth additional probing.
            self.state = State::SearchComplete;

            if let Some(last_probe_time) = last_probe_time {
                self.arm_pmtu_raise_timer(last_probe_time + PMTU_RAISE_TIMER_DURATION);
            }
        }
    }

    /// Called when an excessive number of packets larger than the BASE_PLPMTU have been lost
    #[inline]
    fn on_black_hole_detected<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> MtuResult {
        self.black_hole_counter = Default::default();
        self.largest_acked_mtu_sized_packet = None;
        // Reset the plpmtu back to the base_plpmtu and notify the congestion controller
        self.plpmtu = self.base_plpmtu;
        congestion_controller.on_mtu_update(
            self.plpmtu,
            &mut congestion_controller::PathPublisher::new(publisher, path_id),
        );
        // Cancel any current probes
        self.state = State::SearchComplete;
        // Arm the PMTU raise timer to try a larger MTU again after a cooling off period
        self.arm_pmtu_raise_timer(now + BLACK_HOLE_COOL_OFF_DURATION);

        publisher.on_mtu_updated(event::builder::MtuUpdated {
            path_id: path_id.into_event(),
            mtu: self.plpmtu,
            cause: MtuUpdatedCause::Blackhole,
            search_complete: self.state.is_search_complete(),
        });

        MtuResult::MtuUpdated(self.plpmtu)
    }

    /// Arm the PMTU Raise Timer if there is still room to increase the
    /// MTU before hitting the max plpmtu
    #[inline]
    fn arm_pmtu_raise_timer(&mut self, timestamp: Timestamp) {
        // Reset the max_probe_size to the max_udp_payload to allow for larger probe sizes
        self.max_probe_size = self.max_udp_payload;
        self.update_probed_size();

        if self.is_next_probe_size_above_threshold() {
            // There is still some room to try a larger MTU again,
            // so arm the pmtu raise timer
            self.pmtu_raise_timer.set(timestamp);
        }
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.pmtu_raise_timer.timers(query)?;

        Ok(())
    }
}

impl transmission::Provider for Controller {
    /// Queries the component for any outgoing frames that need to get sent
    ///
    /// This method assumes that no other data (other than the packet header) has been written
    /// to the supplied `WriteContext`. This necessitates the caller ensuring the probe packet
    /// written by this method to be in its own connection transmission.
    #[inline]
    fn on_transmit<W: transmission::Writer>(&mut self, context: &mut W) {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
        //# When used with an acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
        //# generate PLPMTU probes in this state.
        ensure!(self.state == State::SearchRequested);

        ensure!(context.transmission_mode().is_mtu_probing());

        // Each packet contains overhead in the form of a packet header and an authentication tag.
        // This overhead contributes to the overall size of the packet, so the payload we write
        // to the packet will account for this overhead to reach the target probed size.
        let probe_payload_size =
            self.probed_size as usize - context.header_len() - context.tag_len();

        if context.remaining_capacity() < probe_payload_size {
            // There isn't enough capacity in the buffer to write the datagram we
            // want to probe, so we've reached the maximum pmtu and the search is complete.
            self.state = State::SearchComplete;
            return;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
        //# Endpoints could limit the content of PMTU probes to PING and PADDING
        //# frames, since packets that are larger than the current maximum
        //# datagram size are more likely to be dropped by the network.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-3
        //# Probe loss recovery: It is RECOMMENDED to use probe packets that
        //# do not carry any user data that would require retransmission if
        //# lost.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-4.1
        //# DPLPMTUD MAY choose to use only one of these methods to simplify the
        //# implementation.

        context.write_frame(&frame::Ping);
        let padding_size = probe_payload_size - frame::Ping.encoding_size();
        if let Some(packet_number) = context.write_frame(&frame::Padding {
            length: padding_size,
        }) {
            self.probe_count += 1;
            self.state = State::Searching(packet_number, context.current_time());
        }
    }
}

impl transmission::interest::Provider for Controller {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match self.state {
            State::SearchRequested => query.on_new_data(),
            _ => Ok(()),
        }
    }
}
