// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    event::api::{DatagramDropReason, Subject},
    packet::interceptor::{Datagram, Interceptor},
    path::{LocalAddress, RemoteAddress},
};

fn run_test<F>(mut on_rebind: F)
where
    F: FnMut(SocketAddr) -> SocketAddr + Send + 'static,
{
    let model = Model::default();
    let rtt = Duration::from_millis(10);
    let rebind_rate = rtt * 2;
    // we currently only support 4 migrations
    let rebind_count = 4;

    model.set_delay(rtt / 2);

    let expected_paths = Arc::new(Mutex::new(vec![]));
    let expected_paths_pub = expected_paths.clone();

    let on_socket = move |socket: io::Socket| {
        spawn(async move {
            let mut local_addr = socket.local_addr().unwrap();
            for _ in 0..rebind_count {
                local_addr = on_rebind(local_addr);
                delay(rebind_rate).await;
                if let Ok(mut paths) = expected_paths_pub.lock() {
                    paths.push(local_addr);
                }
                socket.rebind(local_addr);
            }
        });
    };

    let active_paths = recorder::ActivePathUpdated::new();
    let active_path_sub = active_paths.clone();

    test(model, move |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), active_path_sub))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client_io = handle.builder().on_socket(on_socket).build()?;

        let client = Client::builder()
            .with_io(client_io)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();

            stream.send(Bytes::from_static(b"A")).await.unwrap();

            delay(rebind_rate / 2).await;

            for _ in 0..rebind_count {
                stream.send(Bytes::from_static(b"B")).await.unwrap();
                delay(rebind_rate).await;
            }

            stream.finish().unwrap();

            let chunk = stream
                .receive()
                .await
                .unwrap()
                .expect("a chunk should be available");
            assert_eq!(&chunk[..], &b"ABBBB"[..]);

            assert!(
                stream.receive().await.unwrap().is_none(),
                "stream should be finished"
            );
        });

        Ok(addr)
    })
    .unwrap();

    assert_eq!(
        &*active_paths.events().lock().unwrap(),
        &*expected_paths.lock().unwrap()
    );
}

/// Ensures that a client that changes its port immediately after
/// sending a handshake packet that completes the handshake succeeds.
#[test]
fn rebind_after_handshake_confirmed() {
    let model = Model::default();

    test(model, move |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .with_packet_interceptor(RebindPortBeforeLastHandshakePacket::default())?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}

// Changes the port of the second handshake packet received
#[derive(Default)]
struct RebindPortBeforeLastHandshakePacket {
    datagram_count: usize,
    handshake_packet_count: usize,
    changed_port: bool,
}

impl Interceptor for RebindPortBeforeLastHandshakePacket {
    // Change the port after the first Handshake packet is received
    fn intercept_rx_remote_address(&mut self, _subject: &Subject, addr: &mut RemoteAddress) {
        if self.handshake_packet_count == 1 && !self.changed_port {
            let port = addr.port();
            addr.set_port(port + 1);
            self.changed_port = true;
        }
    }

    // Drop the first handshake packet from the client (contained within the second
    // datagram the client sends) so that the client sends two Handshake PTO packets
    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        self.datagram_count += 1;
        if self.datagram_count == 2 {
            return DecoderBufferMut::new(&mut payload.into_less_safe_slice()[..0]);
        }
        payload
    }

    // Remove the `ACK` frame from the first two Handshake packets received from the
    // peer, so the first Handshake packet the server transmitted remains in the server's
    // sent packets
    fn intercept_rx_payload<'a>(
        &mut self,
        _subject: &Subject,
        packet: &s2n_quic_core::packet::interceptor::Packet,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        if packet.number.space().is_handshake() {
            self.handshake_packet_count += 1;

            if self.handshake_packet_count <= 2 {
                return DecoderBufferMut::new(&mut payload.into_less_safe_slice()[8..]);
            }
        }

        payload
    }
}

/// Rebinds the IP of an address
fn rebind_ip(mut addr: SocketAddr) -> SocketAddr {
    let ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            let mut v = u32::from_be_bytes(ip.octets());
            v += 1;
            std::net::Ipv4Addr::from(v).into()
        }
        std::net::IpAddr::V6(ip) => {
            let mut v = u128::from_be_bytes(ip.octets());
            v += 1;
            std::net::Ipv6Addr::from(v).into()
        }
    };
    addr.set_ip(ip);
    addr
}

/// Rebinds the port of an address
fn rebind_port(mut addr: SocketAddr) -> SocketAddr {
    let port = addr.port() + 1;
    addr.set_port(port);
    addr
}

#[test]
fn ip_rebind_test() {
    run_test(rebind_ip);
}

#[test]
fn port_rebind_test() {
    run_test(rebind_port);
}

#[test]
fn ip_and_port_rebind_test() {
    run_test(|addr| rebind_ip(rebind_port(addr)));
}

// Changes the port of the second datagram received
#[derive(Default)]
struct RebindPortBeforeHandshakeConfirmed {
    datagram_count: usize,
    changed_port: bool,
}

const REBIND_PORT: u16 = 55555;
impl Interceptor for RebindPortBeforeHandshakeConfirmed {
    fn intercept_rx_remote_address(&mut self, _subject: &Subject, addr: &mut RemoteAddress) {
        if self.datagram_count == 1 && !self.changed_port {
            addr.set_port(REBIND_PORT);
            self.changed_port = true;
        }
    }

    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        self.datagram_count += 1;
        payload
    }
}

/// Ensures that a datagram received from a client that changes
/// its port before the handshake is confirmed is dropped.
#[test]
fn rebind_before_handshake_confirmed() {
    let model = Model::default();
    let subscriber = recorder::DatagramDropped::new();
    let datagram_dropped_events = subscriber.events();

    test(model, move |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?
            .with_packet_interceptor(RebindPortBeforeHandshakeConfirmed::default())?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();

    let datagram_dropped_events = datagram_dropped_events.lock().unwrap();

    assert_eq!(1, datagram_dropped_events.len());
    let event = datagram_dropped_events.first().unwrap();
    assert!(matches!(
        event.reason,
        DatagramDropReason::ConnectionMigrationDuringHandshake { .. },
    ));
    assert_eq!(REBIND_PORT, event.remote_addr.port());
}

// Changes the remote address to ipv4-mapped after the first packet
#[derive(Default)]
struct RebindMappedAddrBeforeHandshakeConfirmed {
    local: bool,
    remote: bool,
    datagram_count: usize,
}

impl Interceptor for RebindMappedAddrBeforeHandshakeConfirmed {
    fn intercept_rx_local_address(&mut self, _subject: &Subject, addr: &mut LocalAddress) {
        if self.datagram_count > 0 && self.local {
            *addr = (*addr).to_ipv6_mapped().into();
        }
    }

    fn intercept_rx_remote_address(&mut self, _subject: &Subject, addr: &mut RemoteAddress) {
        if self.datagram_count > 0 && self.remote {
            *addr = (*addr).to_ipv6_mapped().into();
        }
    }

    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        self.datagram_count += 1;
        payload
    }
}

/// Ensures that a datagram received from a client that changes from ipv4 to ipv4-mapped
/// is still accepted
#[test]
fn rebind_ipv4_mapped_before_handshake_confirmed() {
    fn run_test(interceptor: RebindMappedAddrBeforeHandshakeConfirmed) {
        let model = Model::default();
        let subscriber = recorder::DatagramDropped::new();
        let datagram_dropped_events = subscriber.events();

        test(model, move |handle| {
            let server = Server::builder()
                .with_io(handle.builder().build()?)?
                .with_tls(SERVER_CERTS)?
                .with_event((tracing_events(), subscriber))?
                .with_random(Random::with_seed(456))?
                .with_packet_interceptor(interceptor)?
                .start()?;

            let client = Client::builder()
                .with_io(handle.builder().build()?)?
                .with_tls(certificates::CERT_PEM)?
                .with_event(tracing_events())?
                .with_random(Random::with_seed(456))?
                .start()?;

            let addr = start_server(server)?;
            start_client(client, addr, Data::new(1000))?;
            Ok(addr)
        })
        .unwrap();

        let datagram_dropped_events = datagram_dropped_events.lock().unwrap();
        let datagram_dropped_events = &datagram_dropped_events[..];

        assert!(
            datagram_dropped_events.is_empty(),
            "s2n-quic should not drop IPv4-mapped packets {datagram_dropped_events:?}"
        );
    }

    // test all combinations
    for local in [false, true] {
        for remote in [false, true] {
            let interceptor = RebindMappedAddrBeforeHandshakeConfirmed {
                local,
                remote,
                ..Default::default()
            };
            run_test(interceptor);
        }
    }
}
