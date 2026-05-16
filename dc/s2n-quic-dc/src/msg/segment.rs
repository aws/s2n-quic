// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_platform::features;

/// The maximum size of a IP+UDP header
const IPV4_HEADER_LEN: u16 = 20;
const IPV6_HEADER_LEN: u16 = 40;
const UDP_HEADER_LEN: u16 = 8;

const fn min_u16(a: u16, b: u16) -> u16 {
    if a < b {
        a
    } else {
        b
    }
}

pub const MAX_UDP_PAYLOAD: u16 = 9001 - IPV4_HEADER_LEN - UDP_HEADER_LEN;

/// The maximum number of segments in sendmsg calls
///
/// From <https://elixir.bootlin.com/linux/v6.8.7/source/include/uapi/linux/uio.h#L28>
/// > #define UIO_MAXIOV  1024
pub const MAX_COUNT: usize = if features::gso::IS_SUPPORTED {
    // base the max segments on the max datagram size for the default ethernet mtu
    let max_datagram_size = 1500 - min_u16(IPV4_HEADER_LEN, IPV6_HEADER_LEN) - UDP_HEADER_LEN;

    (MAX_TOTAL / max_datagram_size) as _
} else {
    // only a single segment can be sent per syscall
    1
};

/// The maximum payload allowed in sendmsg calls using IPv4+UDP
const MAX_TOTAL_IPV4: u16 = if cfg!(target_os = "linux") {
    // From <https://github.com/torvalds/linux/blob/8cd26fd90c1ad7acdcfb9f69ca99d13aa7b24561/net/ipv4/ip_output.c#L987-L995>
    // > Linux enforces a u16::MAX - IP_HEADER_LEN - UDP_HEADER_LEN
    u16::MAX - IPV4_HEADER_LEN - UDP_HEADER_LEN
} else {
    9001 - IPV4_HEADER_LEN - UDP_HEADER_LEN
};

/// The maximum payload allowed in sendmsg calls using IPv6+UDP
const MAX_TOTAL_IPV6: u16 = if cfg!(target_os = "linux") {
    // The IPV6_HEADER_LEN is required in this calculation to accommodate older kernels (such as kernel 5.10)
    u16::MAX - IPV6_HEADER_LEN - UDP_HEADER_LEN
} else {
    9001 - IPV6_HEADER_LEN - UDP_HEADER_LEN
};

/// The minimum payload size between the IPv4 and IPv6 sizes
pub const MAX_TOTAL: u16 = min_u16(MAX_TOTAL_IPV4, MAX_TOTAL_IPV6);

#[test]
fn max_total_test() {
    let tests = [("127.0.0.1:0", MAX_TOTAL_IPV4), ("[::1]:0", MAX_TOTAL_IPV6)];

    for (addr, total) in tests {
        let socket = std::net::UdpSocket::bind(addr).unwrap();
        let addr = socket.local_addr().unwrap();

        let mut buffer = vec![0u8; total as usize + 1];

        // This behavior may not be consistent across kernel versions so the check is disabled by default
        let _ = socket.send_to(&buffer, addr);

        buffer.pop().unwrap();
        socket
            .send_to(&buffer, addr)
            .expect("send should succeed when limited to MAX_TOTAL");
    }
}
