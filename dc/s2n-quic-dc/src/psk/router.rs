// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_platform::bpf::cbpf::{abs, and, jeq, ldb, ret, Program};

/// cBPF program to route QUIC packets across multiple sockets.
/// Routes Initial packets with DCID length = 8 to socket 0, all other packets to socket 1.
pub(super) static ROUTER: Program = Program::new(&[
    // Load byte 0 and check if it's an Initial packet (first 4 bits = 1100)
    ldb(abs(0)),
    and(0b1111_0000), // Mask the last four bits of the first byte. The first four bits can confirm if the packet is a INITIAL packet.
    // If Initial packet, continue; else jump to ret(0)
    jeq(0b1100_0000, 0, 2), // First four bits of INITIAL packet should be 1100.
    // Load byte 5 (DCID length) and check if it equals 8
    ldb(abs(5)),
    // If DCID len = 8, jump to ret(1); else continue to ret(0)
    jeq(0x08, 1, 0),
    // Return 0: socket 0 handles Initial packets with DCID length = 8
    ret(0),
    // Return 1: socket 1 handles all other packets
    ret(1),
]);

#[cfg(test)]
mod test {
    use super::*;
    use s2n_quic_platform::syscall;
    use std::io;
    use tokio::{net::UdpSocket, time::Duration};

    // Tests that the ROUTER cBPF filter correctly routes packets to the appropriate socket.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn router_cbpf_packet_filtering_test() -> io::Result<()> {
        static IPV4_LOCALHOST: &str = "127.0.0.1:0";

        // Create two rx sockets bound to same port with SO_REUSEPORT
        let rx_socket_0 = syscall::bind_udp(IPV4_LOCALHOST, false, false, false)?;
        rx_socket_0.set_nonblocking(true)?;
        let port = rx_socket_0.local_addr()?.as_socket().unwrap().port();
        rx_socket_0.set_reuse_port(true)?;

        let rx_socket_1 = syscall::bind_udp(("127.0.0.1", port), false, true, false)?;
        rx_socket_1.set_nonblocking(true)?;

        // Attach ROUTER to both sockets
        ROUTER.attach(&rx_socket_0)?;
        ROUTER.attach(&rx_socket_1)?;

        // Convert to tokio sockets for async recv
        let rx_socket_0 = UdpSocket::from_std(rx_socket_0.into())?;
        let rx_socket_1 = UdpSocket::from_std(rx_socket_1.into())?;

        // Create sender socket
        let sender = UdpSocket::bind("127.0.0.1:0").await?;
        let target_addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        // Test packet A: Initial packet with DCID length = 8
        // Should route to socket 1
        // Format: [header byte, version (4 bytes), dcid_len, ...]
        let packet_a = {
            let mut p = vec![0u8; 32];
            p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
            p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
            p[5] = 0x08; // DCID length = 8
            p
        };

        // Test packet B: Handshake packet
        // Should route to socket 0
        let packet_b = {
            let mut p = vec![0u8; 32];
            p[0] = 0xE0; // Handshake packet first four bits are 1110
            p
        };

        // Test packet C: Initial packet but DCID length != 8
        // Should route to socket 0
        let packet_c = {
            let mut p = vec![0u8; 32];
            p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
            p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
            p[5] = 0x10; // DCID length = 16
            p
        };

        // Send packets
        sender.send_to(&packet_a, target_addr).await?;
        sender.send_to(&packet_b, target_addr).await?;
        sender.send_to(&packet_c, target_addr).await?;

        // Receive and verify routing
        let mut buf_socket1 = [0u8; 1024];
        let mut buf_packet1_socket0 = [0u8; 1024];
        let mut buf_packet2_socket0 = [0u8; 1024];

        // Socket 1 should receive packet_a (Initial with DCID len = 8)
        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_1.recv_from(&mut buf_socket1),
        )
        .await;
        let (len, _) = recv_result.unwrap()?;
        assert_eq!(&buf_socket1[..len], &packet_a);

        // Socket 1 should receive packet_b and packet_c
        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_0.recv_from(&mut buf_packet1_socket0),
        )
        .await;
        let (len1, _) = recv_result.unwrap()?;

        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_0.recv_from(&mut buf_packet2_socket0),
        )
        .await;
        let (len2, _) = recv_result.unwrap()?;

        // Verify that socket 1 received exactly packet_b and packet_c in either order
        let received_packets = [&buf_packet1_socket0[..len1], &buf_packet2_socket0[..len2]];

        // Check that one packet matches packet_b (Handshake: header = 0xE0)
        let has_packet_b = received_packets.iter().any(|p| p[..] == packet_b[..]);
        assert!(
            has_packet_b,
            "Socket 1 should receive packet_b (Handshake packet with header 0xE0)"
        );

        // Check that one packet matches packet_c (Initial with DCID len = 16)
        let has_packet_c = received_packets.iter().any(|p| p[..] == packet_c[..]);
        assert!(
            has_packet_c,
            "Socket 1 should receive packet_c (Initial packet with DCID len=16)"
        );

        // Socket 0 should not have any more packets
        let recv_result = tokio::time::timeout(
            Duration::from_millis(100),
            rx_socket_0.recv_from(&mut buf_socket1),
        )
        .await;
        assert!(
            recv_result.is_err(),
            "Socket 0 should not receive any more packets"
        );

        Ok(())
    }
}
