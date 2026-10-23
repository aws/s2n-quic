// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Hands dcQUIC streams over UDP off to an application process.
//!
//! The manager validates each new stream's first packet, then sends the stream's secret and that
//! packet to the application over a Unix domain socket. The application replies from its own UDP
//! sockets and the client follows the new port. Until then the client keeps sending to the
//! manager, so later packets for a handed-off stream are forwarded to the application instead of
//! being validated again, which would flag them as replays.
//!
//! The manager never sends a stream or control packet itself; those would pin the client to the
//! manager's port. It only sends secret control packets, which the client handles without
//! changing its peer.

use crate::{
    credentials::{self, Credentials},
    crypto::{self, open::Application as _},
    event::{self, EndpointPublisher as _},
    packet::{self, stream},
    path::secret::{self, map::Bidirectional, schedule::Ciphersuite},
    socket::recv::{descriptor, router::Router},
    stream::{environment::Environment, TransportFeatures},
    uds::sender::Sender,
};
use nix::{sys::time::TimeValLike as _, time::ClockId};
use s2n_codec::{EncoderBuffer, EncoderLenEstimator};
use s2n_quic_core::{
    dc::ApplicationParams,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::{Clock as _, Timestamp},
    varint::VarInt,
};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    io,
    net::UdpSocket,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

/// The most handed-off streams each socket remembers.
const MAX_HANDOFFS: usize = 16_384;

/// How long a handoff is remembered when the stream has no idle timeout.
const DEFAULT_HANDOFF_LIFETIME: Duration = Duration::from_secs(30);

type Key = (credentials::Id, u64);

#[inline]
fn key(credentials: &Credentials) -> Key {
    (credentials.id, credentials.key_id.as_u64())
}

struct Handoff {
    ciphersuite: Ciphersuite,
    export_secret: secret::schedule::ExportSecret,
    application_params: ApplicationParams,
    application_data: Option<Vec<u8>>,
    /// Set once the application has the stream; until then, retransmits resend the handoff
    delivered: bool,
    expires_at: Timestamp,
}

/// Streams handed off from one socket, bounded in size and lifetime.
///
/// A client sends to the manager only until it sees the application's first reply, which takes
/// at most the stream's idle timeout, so entries expire then. If the table is full the oldest
/// entry is evicted; a later packet for that stream is validated again and dropped as a replay.
#[derive(Default)]
struct Handoffs {
    entries: HashMap<Key, Handoff>,
    order: VecDeque<(Key, Timestamp)>,
}

impl Handoffs {
    fn get_mut(&mut self, key: &Key, now: Timestamp) -> Option<&mut Handoff> {
        self.expire(now);
        self.entries
            .get_mut(key)
            .filter(|handoff| handoff.expires_at > now)
    }

    fn insert(&mut self, key: Key, handoff: Handoff, now: Timestamp) {
        self.expire(now);
        while self.entries.len() >= MAX_HANDOFFS {
            let Some((oldest, _)) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back((key, handoff.expires_at));
        self.entries.insert(key, handoff);
    }

    fn expire(&mut self, now: Timestamp) {
        while let Some((key, expires_at)) = self.order.front() {
            if *expires_at > now {
                break;
            }
            if let Entry::Occupied(entry) = self.entries.entry(*key) {
                if entry.get().expires_at <= now {
                    entry.remove();
                }
            }
            self.order.pop_front();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

enum Pending {
    Drop,
    Handoff {
        key: Key,
        credentials: Credentials,
        remote_address: SocketAddress,
    },
    Forward {
        credentials: Credentials,
        remote_address: SocketAddress,
    },
}

pub struct Acceptor<Env: Environment> {
    env: Env,
    secrets: secret::Map,
    socket: Arc<UdpSocket>,
    local_address: SocketAddress,
    socket_path: PathBuf,
    sender: Option<Sender>,
    handoffs: Handoffs,
    pending: Pending,
}

impl<Env: Environment> Acceptor<Env> {
    pub fn new(
        env: Env,
        secrets: secret::Map,
        socket: Arc<UdpSocket>,
        socket_path: PathBuf,
    ) -> io::Result<Self> {
        let local_address = socket.local_addr()?.into();
        let sender = Some(Sender::new(&socket_path)?);
        Ok(Self {
            env,
            secrets,
            socket,
            local_address,
            socket_path,
            sender,
            handoffs: Handoffs::default(),
            pending: Pending::Drop,
        })
    }

    fn on_initial_packet(
        &mut self,
        remote_address: SocketAddress,
        packet: &stream::decoder::Packet,
    ) -> Pending {
        let credentials = *packet.credentials();
        let key = key(&credentials);
        let now = self.env.clock().get_time();

        if let Some(handoff) = self.handoffs.get_mut(&key, now) {
            return if handoff.delivered {
                Pending::Forward {
                    credentials,
                    remote_address,
                }
            } else {
                Pending::Handoff {
                    key,
                    credentials,
                    remote_address,
                }
            };
        }

        let mut secret_control = vec![];
        let Some((export_secret, ciphersuite, keys, application_params, application_data)) =
            self.secrets.secret_for_credentials(
                &credentials,
                packet.source_queue_id(),
                &TransportFeatures::UDP,
                &mut secret_control,
            )
        else {
            if !secret_control.is_empty() {
                let _ = self
                    .socket
                    .send_to(&secret_control, std::net::SocketAddr::from(remote_address));
            }
            return Pending::Drop;
        };

        // Decrypting enforces the path secret's replay protection here, since the application
        // trusts the manager to have checked.
        if let Err(error) = decrypt(keys, packet) {
            tracing::debug!(?error, "dropping undecryptable initial packet");
            return Pending::Drop;
        }

        let application_data = match application_data
            .as_ref()
            .map(|data| self.secrets.serialize_application_data(data))
        {
            None | Some(Ok(None)) => None,
            Some(Ok(Some(bytes))) => Some(bytes),
            Some(Err(err)) => {
                tracing::warn!(
                    ?err,
                    "failed to serialize application data; forwarding without it"
                );
                None
            }
        };

        let lifetime = application_params
            .max_idle_timeout()
            .unwrap_or(DEFAULT_HANDOFF_LIFETIME);
        let handoff = Handoff {
            ciphersuite,
            export_secret,
            application_params,
            application_data,
            delivered: false,
            expires_at: now + lifetime,
        };
        self.handoffs.insert(key, handoff, now);

        Pending::Handoff {
            key,
            credentials,
            remote_address,
        }
    }

    fn handoff(
        &mut self,
        key: Key,
        credentials: Credentials,
        remote_address: SocketAddress,
        segment: &descriptor::Filled,
    ) {
        let now = self.env.clock().get_time();
        let Ok(encode_time) = encode_time() else {
            return;
        };
        let local_address = self.local_address;
        let Some(handoff) = self.handoffs.get_mut(&key, now) else {
            return;
        };
        let packet = encode_handoff(
            handoff,
            encode_time,
            &remote_address,
            &local_address,
            segment.payload(),
        );

        if self.send(&packet) {
            if let Some(handoff) = self.handoffs.get_mut(&key, now) {
                handoff.delivered = true;
            }
            self.env
                .endpoint_publisher_with_time(now)
                .on_acceptor_udp_stream_handed_off(event::builder::AcceptorUdpStreamHandedOff {
                    remote_address: &remote_address,
                    credential_id: &*credentials.id,
                    stream_id: credentials.key_id.as_u64(),
                    payload_len: packet.len(),
                });
        }
    }

    fn forward(
        &mut self,
        credentials: Credentials,
        remote_address: SocketAddress,
        segment: &descriptor::Filled,
    ) {
        let Ok(encode_time) = encode_time() else {
            return;
        };
        let payload = segment.payload();
        let size = packet::uds::encoder::encode_forward(
            &mut EncoderLenEstimator::new(usize::MAX),
            encode_time,
            &remote_address,
            payload,
        );
        let mut buffer = vec![0u8; size];
        packet::uds::encoder::encode_forward(
            &mut EncoderBuffer::new(&mut buffer),
            encode_time,
            &remote_address,
            payload,
        );

        if self.send(&buffer) {
            let now = self.env.clock().get_time();
            self.env
                .endpoint_publisher_with_time(now)
                .on_acceptor_udp_packet_forwarded(event::builder::AcceptorUdpPacketForwarded {
                    remote_address: &remote_address,
                    credential_id: &*credentials.id,
                    stream_id: credentials.key_id.as_u64(),
                });
        }
    }

    /// Sends `packet` to the application without blocking, returning `true` if it was queued.
    ///
    /// A full queue drops the packet; the client retransmits it. If the application restarted,
    /// its socket is reconnected once.
    fn send(&mut self, packet: &[u8]) -> bool {
        let mut result = match &self.sender {
            Some(sender) => sender.try_send(packet),
            None => Err(io::ErrorKind::NotConnected.into()),
        };

        if matches!(
            result.as_ref().map_err(io::Error::kind),
            Err(io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotFound
                | io::ErrorKind::NotConnected)
        ) {
            self.sender = Sender::new(&self.socket_path).ok();
            if let Some(sender) = &self.sender {
                result = sender.try_send(packet);
            }
        }

        match result {
            Ok(()) => true,
            Err(error) => {
                let now = self.env.clock().get_time();
                self.env
                    .endpoint_publisher_with_time(now)
                    .on_acceptor_udp_io_error(event::builder::AcceptorUdpIoError { error: &error });
                false
            }
        }
    }
}

impl<Env: Environment> Router for Acceptor<Env> {
    #[inline]
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        remote_address: SocketAddress,
        _ecn: ExplicitCongestionNotification,
        packet: stream::decoder::Packet,
    ) {
        // Packets with a queue ID are for a stream the client already follows to the application.
        self.pending = if packet.stream_id().queue_id() == VarInt::ZERO {
            self.on_initial_packet(remote_address, &packet)
        } else {
            Pending::Drop
        };
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: stream::Tag,
        _id: stream::Id,
        _credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        match core::mem::replace(&mut self.pending, Pending::Drop) {
            Pending::Drop => {}
            Pending::Handoff {
                key,
                credentials,
                remote_address,
            } => self.handoff(key, credentials, remote_address, &segment),
            Pending::Forward {
                credentials,
                remote_address,
            } => self.forward(credentials, remote_address, &segment),
        }
    }
}

fn encode_handoff(
    handoff: &Handoff,
    encode_time: u64,
    remote_address: &SocketAddress,
    local_address: &SocketAddress,
    payload: &[u8],
) -> Vec<u8> {
    let size = |application_data: Option<&[u8]>| {
        packet::uds::encoder::encode_v2(
            &mut EncoderLenEstimator::new(usize::MAX),
            packet::uds::Protocol::Udp,
            &handoff.ciphersuite,
            &handoff.export_secret,
            &handoff.application_params,
            encode_time,
            remote_address,
            Some(local_address),
            application_data,
            payload,
        )
    };

    let mut application_data = handoff.application_data.as_deref();
    let mut len = size(application_data);
    // Unix datagrams cannot exceed `u16::MAX` bytes, so forward without the application data
    // rather than failing the stream.
    if len > u16::MAX as usize && application_data.is_some() {
        tracing::warn!(
            len,
            "UDS handoff packet exceeds datagram buffer with application data; forwarding without it"
        );
        application_data = None;
        len = size(application_data);
    }

    let mut buffer = vec![0u8; len];
    packet::uds::encoder::encode_v2(
        &mut EncoderBuffer::new(&mut buffer),
        packet::uds::Protocol::Udp,
        &handoff.ciphersuite,
        &handoff.export_secret,
        &handoff.application_params,
        encode_time,
        remote_address,
        Some(local_address),
        application_data,
        payload,
    );
    buffer
}

fn decrypt(keys: Bidirectional, packet: &stream::decoder::Packet) -> crypto::open::Result {
    let mut payload_out = vec![0u8; packet.payload().len()];
    let payload_out = crypto::UninitSlice::new(&mut payload_out);
    keys.application.opener.decrypt(
        packet.tag().key_phase(),
        *packet.packet_number(),
        packet.header(),
        packet.payload(),
        packet.auth_tag(),
        payload_out,
    )
}

fn encode_time() -> io::Result<u64> {
    #[cfg(target_os = "linux")]
    let clock = ClockId::CLOCK_MONOTONIC_RAW;

    #[cfg(not(target_os = "linux"))]
    let clock = ClockId::CLOCK_MONOTONIC;

    Ok(clock.now().map_err(io::Error::from)?.num_microseconds() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::time::{testing::Clock as TestClock, Clock};

    fn handoff(expires_at: Timestamp) -> Handoff {
        Handoff {
            ciphersuite: Ciphersuite::AES_GCM_128_SHA256,
            export_secret: [0; 32],
            application_params: s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS,
            application_data: None,
            delivered: false,
            expires_at,
        }
    }

    fn test_key(n: u64) -> Key {
        (credentials::Id::default(), n)
    }

    #[test]
    fn handoffs_expire() {
        let mut clock = TestClock::default();
        let now = clock.get_time();
        let mut handoffs = Handoffs::default();
        handoffs.insert(test_key(1), handoff(now + Duration::from_secs(1)), now);
        assert!(handoffs.get_mut(&test_key(1), now).is_some());

        clock.inc_by(Duration::from_secs(1));
        assert!(handoffs.get_mut(&test_key(1), clock.get_time()).is_none());
        assert_eq!(handoffs.len(), 0);
    }

    #[test]
    fn handoffs_are_bounded() {
        let clock = TestClock::default();
        let now = clock.get_time();
        let expires_at = now + Duration::from_secs(60);
        let mut handoffs = Handoffs::default();
        for n in 0..=MAX_HANDOFFS as u64 {
            handoffs.insert(test_key(n), handoff(expires_at), now);
        }
        assert_eq!(handoffs.len(), MAX_HANDOFFS);
        assert!(handoffs.get_mut(&test_key(0), now).is_none());
        assert!(handoffs
            .get_mut(&test_key(MAX_HANDOFFS as u64), now)
            .is_some());
    }
}
