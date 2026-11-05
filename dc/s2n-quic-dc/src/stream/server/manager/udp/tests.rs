// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    packet::uds::decoder,
    path::secret::stateless_reset,
    socket::recv::pool::Pool,
    stream::{environment::tokio as env, TransportFeatures},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{buffer::reader, time::NoopClock};
use std::{
    net::SocketAddr,
    num::NonZeroU32,
    os::unix::net::UnixDatagram,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct Counts {
    handoffs: Arc<AtomicUsize>,
    forwards: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
}

impl event::Subscriber for Counts {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_acceptor_udp_stream_handed_off(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpStreamHandedOff,
    ) {
        self.handoffs.fetch_add(1, Ordering::Relaxed);
    }

    fn on_acceptor_udp_packet_forwarded(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpPacketForwarded,
    ) {
        self.forwards.fetch_add(1, Ordering::Relaxed);
    }

    fn on_acceptor_udp_io_error(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpIoError,
    ) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

struct Fixture {
    manager: Acceptor<env::Environment<Counts>>,
    client_map: secret::Map,
    client_socket: UdpSocket,
    manager_addr: SocketAddr,
    receiver: UnixDatagram,
    packets: Pool,
    counts: Counts,
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new(idle_timeout_ms: Option<u32>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("application.sock");
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver.set_nonblocking(true).unwrap();
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_socket.set_nonblocking(true).unwrap();
        let manager_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let manager_addr = manager_socket.local_addr().unwrap();
        let counts = Counts::default();
        let map = || {
            let map = secret::Map::new(
                stateless_reset::Signer::random(),
                10,
                false,
                NoopClock,
                counts.clone(),
            );
            map.test_stop_cleaner();
            map
        };
        let client_map = map();
        let server_map = map();
        let mut params = s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS;
        params.max_idle_timeout = idle_timeout_ms.and_then(NonZeroU32::new);
        client_map.test_insert_pair(
            client_socket.local_addr().unwrap(),
            Some(params.clone()),
            &server_map,
            manager_addr,
            Some(params),
        );
        let environment = env::Builder::new(counts.clone())
            .with_threads(1)
            .build()
            .unwrap();
        let manager = Acceptor::new(environment, server_map, manager_socket, path).unwrap();
        Self {
            manager,
            client_map,
            client_socket,
            manager_addr,
            receiver,
            packets: Pool::new(1500, 1),
            counts,
            directory,
        }
    }

    fn keys(&self) -> Bidirectional {
        self.client_map
            .get_untracked(self.manager_addr)
            .unwrap()
            .pair(&TransportFeatures::UDP)
            .unwrap()
            .0
    }

    fn packet(&self) -> Vec<u8> {
        stream_packet(&self.keys(), VarInt::ZERO, VarInt::ZERO)
    }

    fn dispatch(&mut self, packet: &[u8]) {
        let address = self.client_socket.local_addr().unwrap().into();
        let mut segments = self
            .packets
            .alloc_or_grow()
            .recv_with(|addr, _cmsg, mut buffer| {
                addr.set(address);
                buffer[..packet.len()].copy_from_slice(packet);
                Ok::<_, io::Error>(packet.len())
            })
            .map_err(|(_, error)| error)
            .unwrap();
        self.manager.on_segment(segments.next().unwrap());
    }

    fn receive(&self) -> Vec<u8> {
        let mut buffer = vec![0; u16::MAX as usize];
        let len = self.receiver.recv(&mut buffer).unwrap();
        buffer.truncate(len);
        buffer
    }

    fn assert_handoff(&self, packet: &[u8]) {
        let mut message = self.receive();
        assert!(!decoder::Forward::is_forward(&message));
        let (handoff, remaining) =
            decoder::Packet::decode(DecoderBufferMut::new(&mut message)).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(handoff.protocol(), packet::uds::Protocol::Udp);
        assert_eq!(handoff.payload(), packet);
        assert_eq!(
            handoff.peer_address(),
            Some(&self.client_socket.local_addr().unwrap().into())
        );
        assert_eq!(handoff.local_address(), Some(&self.manager_addr.into()));
    }

    fn assert_forward(&self, packet: &[u8]) {
        let mut message = self.receive();
        assert!(decoder::Forward::is_forward(&message));
        let (forward, remaining) =
            decoder::Forward::decode(DecoderBufferMut::new(&mut message)).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(forward.payload(), packet);
        assert_eq!(
            *forward.peer_address(),
            self.client_socket.local_addr().unwrap().into()
        );
    }

    fn assert_empty(&self) {
        let error = self.receiver.recv(&mut [0; 1]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    }

    fn assert_no_secret_control(&self) {
        let error = self.client_socket.recv_from(&mut [0; 128]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    }

    fn fill_queue(&self) -> usize {
        let sender = self.manager.sender.as_ref().unwrap();
        for count in 0..65_536 {
            match sender.try_send(b"queue filler") {
                Ok(()) => {}
                Err(error) => {
                    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                    assert!(count > 0);
                    return count;
                }
            }
        }
        panic!("Unix datagram queue did not fill");
    }

    fn drain_fillers(&self, count: usize) {
        for _ in 0..count {
            assert_eq!(self.receive(), b"queue filler");
        }
    }

    async fn wait_for_delivery_capacity(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.manager.delivery_capacity.available_permits() != MAX_PENDING_HANDOFFS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    fn restart_application(&mut self) {
        let path = self.directory.path().join("application.sock");
        let replacement = UnixDatagram::unbound().unwrap();
        drop(std::mem::replace(&mut self.receiver, replacement));
        std::fs::remove_file(&path).unwrap();
        self.receiver = UnixDatagram::bind(&path).unwrap();
        self.receiver.set_nonblocking(true).unwrap();
    }
}

fn stream_packet(keys: &Bidirectional, queue_id: VarInt, packet_number: VarInt) -> Vec<u8> {
    let mut payload: &[u8] = b"stream payload";
    let mut reader = reader::Complete::new(&mut payload).unwrap();
    let mut bytes = vec![0; 1500];
    let len = stream::encoder::encode(
        EncoderBuffer::new(&mut bytes),
        Some(17u8.into()),
        stream::Id::normal(queue_id).unwrap(),
        packet_number,
        VarInt::ZERO,
        VarInt::ZERO,
        &mut &[][..],
        VarInt::ZERO,
        &(),
        &mut reader,
        &keys.application.sealer,
        &keys.credentials,
    );
    bytes.truncate(len);
    bytes
}

fn control_packet(keys: &Bidirectional) -> Vec<u8> {
    let mut bytes = vec![0; 1500];
    let len = packet::control::encoder::encode(
        EncoderBuffer::new(&mut bytes),
        Some(17u8.into()),
        Some(stream::Id::normal(7u8.into()).unwrap()),
        2u8.into(),
        VarInt::ZERO,
        &mut &[][..],
        VarInt::ZERO,
        &(),
        &keys.control.as_ref().unwrap().sealer,
        &keys.credentials,
    );
    bytes.truncate(len);
    bytes
}

fn probe_packet(keys: &Bidirectional) -> Vec<u8> {
    let mut payload: &[u8] = &[];
    let mut reader = reader::Complete::new(&mut payload).unwrap();
    let mut bytes = vec![0; 1500];
    let len = stream::encoder::probe(
        EncoderBuffer::new(&mut bytes),
        Some(17u8.into()),
        stream::Id::normal(VarInt::ZERO).unwrap(),
        VarInt::ZERO,
        VarInt::ZERO,
        VarInt::ZERO,
        &mut &[][..],
        VarInt::ZERO,
        &(),
        &mut reader,
        &keys.control.as_ref().unwrap().sealer,
        &keys.credentials,
    );
    bytes.truncate(len);
    bytes
}

#[tokio::test]
async fn retransmission_of_a_lost_first_packet_can_grant_a_stream() {
    for space in [stream::PacketSpace::Stream, stream::PacketSpace::Recovery] {
        let mut fixture = Fixture::new(None);
        let keys = fixture.keys();
        let original = stream_packet(&keys, VarInt::ZERO, VarInt::ZERO);
        let mut retransmission = original.clone();
        stream::decoder::Packet::retransmit(
            DecoderBufferMut::new(&mut retransmission),
            space,
            7u8.into(),
            &keys.control.as_ref().unwrap().sealer,
        )
        .unwrap();

        fixture.dispatch(&retransmission);
        fixture.assert_handoff(&retransmission);
        assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);

        fixture.dispatch(&retransmission);
        fixture.assert_forward(&retransmission);
        fixture.dispatch(&original);
        fixture.assert_forward(&original);
        assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
        fixture.assert_no_secret_control();
    }
}

#[tokio::test]
async fn a_signed_recovery_probe_cannot_grant_or_consume_an_admission() {
    let mut fixture = Fixture::new(None);
    let keys = fixture.keys();
    let probe = probe_packet(&keys);
    fixture.dispatch(&probe);
    fixture.assert_forward(&probe);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    assert_eq!(
        fixture.manager.delivery_capacity.available_permits(),
        MAX_PENDING_HANDOFFS
    );

    let data = stream_packet(&keys, VarInt::ZERO, 1u8.into());
    fixture.dispatch(&data);
    fixture.assert_handoff(&data);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
    fixture.assert_no_secret_control();
}

#[tokio::test(start_paused = true)]
async fn duplicate_packets_forward_after_the_former_lifetime_cutoff() {
    let mut fixture = Fixture::new(None);
    let packet = fixture.packet();
    fixture.dispatch(&packet);
    fixture.assert_handoff(&packet);
    tokio::time::advance(Duration::from_secs(31)).await;
    fixture.dispatch(&packet);
    fixture.assert_forward(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.counts.forwards.load(Ordering::Relaxed), 1);
    fixture.assert_no_secret_control();
}

#[tokio::test]
async fn old_stream_packets_forward_after_many_other_admissions() {
    let mut fixture = Fixture::new(None);
    let first = fixture.packet();
    fixture.dispatch(&first);
    fixture.assert_handoff(&first);

    for _ in 0..16_385 {
        let other = fixture.packet();
        fixture.dispatch(&other);
        fixture.assert_handoff(&other);
    }

    // Exceed both the replay window and the former 16k handoff limit.
    fixture.dispatch(&first);
    fixture.assert_forward(&first);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 16_386);
    assert_eq!(fixture.counts.forwards.load(Ordering::Relaxed), 1);
    fixture.assert_no_secret_control();
}

#[tokio::test]
async fn nonzero_stream_and_stream_control_packets_only_forward() {
    let mut fixture = Fixture::new(None);
    let keys = fixture.keys();
    let stream = stream_packet(&keys, 7u8.into(), 1u8.into());
    fixture.dispatch(&stream);
    fixture.assert_forward(&stream);
    let control = control_packet(&keys);
    fixture.dispatch(&control);
    fixture.assert_forward(&control);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);

    let initial = stream_packet(&keys, VarInt::ZERO, VarInt::ZERO);
    fixture.dispatch(&initial);
    fixture.assert_handoff(&initial);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
    fixture.assert_no_secret_control();
}

#[tokio::test]
async fn full_delivery_capacity_does_not_consume_a_key() {
    let mut fixture = Fixture::new(None);
    let capacity = fixture
        .manager
        .delivery_capacity
        .clone()
        .try_acquire_many_owned(MAX_PENDING_HANDOFFS as u32)
        .unwrap();
    let packet = fixture.packet();
    fixture.dispatch(&packet);
    fixture.assert_forward(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    drop(capacity);

    fixture.dispatch(&packet);
    fixture.assert_handoff(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
    assert_eq!(
        fixture.manager.delivery_capacity.available_permits(),
        MAX_PENDING_HANDOFFS
    );
}

#[tokio::test]
async fn authentication_failure_releases_capacity_and_leaves_the_key_unclaimed() {
    let mut fixture = Fixture::new(None);
    let valid = fixture.packet();
    let mut invalid = valid.clone();
    *invalid.last_mut().unwrap() ^= 1;
    fixture.dispatch(&invalid);
    fixture.assert_forward(&invalid);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    assert_eq!(
        fixture.manager.delivery_capacity.available_permits(),
        MAX_PENDING_HANDOFFS
    );

    fixture.dispatch(&valid);
    fixture.assert_handoff(&valid);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);
    fixture.assert_no_secret_control();
}

#[tokio::test]
async fn a_full_uds_queue_retries_one_original_handoff() {
    let mut fixture = Fixture::new(None);
    let filler_count = fixture.fill_queue();
    let packet = fixture.packet();
    fixture.dispatch(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    assert_eq!(
        fixture.manager.delivery_capacity.available_permits(),
        MAX_PENDING_HANDOFFS - 1
    );
    // A retransmit cannot create another pending grant, even while delivery is blocked.
    fixture.dispatch(&packet);
    assert_eq!(
        fixture.manager.delivery_capacity.available_permits(),
        MAX_PENDING_HANDOFFS - 1
    );
    fixture.drain_fillers(filler_count);
    fixture.wait_for_delivery_capacity().await;
    fixture.assert_handoff(&packet);
    fixture.assert_empty();
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);

    fixture.dispatch(&packet);
    fixture.assert_forward(&packet);
    fixture.assert_no_secret_control();
}

#[tokio::test(start_paused = true)]
async fn an_expired_unsent_handoff_is_never_granted_again() {
    let mut fixture = Fixture::new(Some(10));
    let filler_count = fixture.fill_queue();
    let packet = fixture.packet();
    fixture.dispatch(&packet);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(11)).await;
    fixture.wait_for_delivery_capacity().await;
    fixture.drain_fillers(filler_count);
    fixture.assert_empty();
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    assert!(fixture.counts.errors.load(Ordering::Relaxed) >= 2);

    fixture.dispatch(&packet);
    fixture.assert_forward(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 0);
    fixture.assert_no_secret_control();
}

#[tokio::test]
async fn an_expired_retry_does_not_enqueue_on_a_writable_socket() {
    let fixture = Fixture::new(None);
    let error = retry_handoff(
        fixture.manager.sender.clone(),
        &fixture.manager.socket_path,
        b"expired handoff",
        tokio::time::Instant::now(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    fixture.assert_empty();
}

#[tokio::test]
async fn application_restart_cannot_regenerate_a_successful_handoff() {
    let mut fixture = Fixture::new(None);
    let packet = fixture.packet();
    fixture.dispatch(&packet);
    fixture.assert_handoff(&packet);
    fixture.restart_application();

    fixture.dispatch(&packet);
    fixture.assert_forward(&packet);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 1);

    let fresh = fixture.packet();
    fixture.dispatch(&fresh);
    fixture.assert_handoff(&fresh);
    assert_eq!(fixture.counts.handoffs.load(Ordering::Relaxed), 2);
    fixture.assert_no_secret_control();
}
