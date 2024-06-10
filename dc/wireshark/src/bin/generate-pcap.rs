// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::{EncoderBuffer, EncoderValue};
use std::{io::Write, net::Ipv4Addr};

const MAGIC_NUMBER: u32 = 0xa1b2c3d4;

fn main() {
    // https://wiki.wireshark.org/Development/LibpcapFileFormat#overview
    let mut output =
        std::io::BufWriter::new(std::fs::File::create("generated-datagrams.pcap").unwrap());

    output.write_all(&MAGIC_NUMBER.to_ne_bytes()).unwrap();

    // version major.minor
    output.write_all(&2u16.to_ne_bytes()).unwrap();
    output.write_all(&4u16.to_ne_bytes()).unwrap();

    // GMT/local timezone conversion
    output.write_all(&0i32.to_ne_bytes()).unwrap();

    // Sigfigs. Wireshark says this should always be zero.
    output.write_all(&0u32.to_ne_bytes()).unwrap();

    let max_length = 500u32;
    // Snaplen, maximum capture length.
    output.write_all(&max_length.to_ne_bytes()).unwrap();

    // Network kind. We're writing ethernet packets.
    output.write_all(&1u32.to_ne_bytes()).unwrap();

    let timestamp_start = 1716923147u32;
    for idx in 0..100_000 {
        output.write_all(&timestamp_start.to_ne_bytes()).unwrap();
        // micros -- we always have zeros for this field.
        output.write_all(&0u32.to_ne_bytes()).unwrap();

        // Non-fragmented packet.
        let mut packet = Packet::new();

        packet.add_ethernet();
        packet.add_ip(None);
        packet.add_udp();
        packet.add_dcquic(idx as u64);

        packet.ip_fill_length();
        packet.udp_fill_length();

        assert!(packet.buffer.len() <= max_length as usize);

        // Captured length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();
        // Real length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();

        output.write_all(&packet.buffer).unwrap();

        output.write_all(&timestamp_start.to_ne_bytes()).unwrap();
        // micros -- we always have zeros for this field.
        output.write_all(&0u32.to_ne_bytes()).unwrap();

        // Then write the same packet split into two parts.
        let mut packet = Packet::new();

        packet.add_ethernet();
        packet.add_ip(Some((0, true)));
        packet.add_udp();
        packet.add_dcquic((1 << 15) | idx as u64);

        // UDP length must be filled before splitting as the UDP length includes both payloads.
        packet.udp_fill_length();

        let tail = packet.buffer.split_off(packet.udp_start.unwrap() + 16);

        packet.ip_fill_length();

        assert!(packet.buffer.len() <= max_length as usize);

        // Captured length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();
        // Real length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();

        output.write_all(&packet.buffer).unwrap();

        output.write_all(&timestamp_start.to_ne_bytes()).unwrap();
        // micros -- we always have zeros for this field.
        output.write_all(&0u32.to_ne_bytes()).unwrap();

        let mut packet = Packet::new();

        packet.add_ethernet();
        packet.add_ip(Some((16, false)));
        packet.buffer.extend(tail);

        packet.ip_fill_length();

        assert!(packet.buffer.len() <= max_length as usize);

        // Captured length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();
        // Real length.
        output
            .write_all(&u32::try_from(packet.buffer.len()).unwrap().to_ne_bytes())
            .unwrap();

        output.write_all(&packet.buffer).unwrap();
    }
}

struct Packet {
    buffer: Vec<u8>,

    ip_start: Option<usize>,
    ip_length_field: Option<usize>,

    udp_start: Option<usize>,
    udp_length_field: Option<usize>,
}

impl Packet {
    fn new() -> Packet {
        Packet {
            buffer: Vec::with_capacity(500),
            ip_start: None,
            ip_length_field: None,
            udp_start: None,
            udp_length_field: None,
        }
    }

    fn add_ethernet(&mut self) {
        // Ethernet and IP headers.
        self.buffer.write_all(&[0; 6]).unwrap();
        self.buffer.write_all(&[0; 6]).unwrap();
        // Ipv4
        self.buffer.write_all(&[0x08, 0x00]).unwrap();
    }

    // fragment is a (offset, more fragments) tuple.
    fn add_ip(&mut self, fragment: Option<(usize, bool)>) {
        self.ip_start = Some(self.buffer.len());

        // 0x05 = 20 byte header.
        self.buffer.write_all(&[0x45]).unwrap();

        // No DSCP or ECN flags.
        self.buffer.write_all(&[0x0]).unwrap();

        self.ip_length_field = Some(self.buffer.len());

        // Total length
        self.buffer.write_all(&0u16.to_be_bytes()).unwrap();

        // Identification field.
        self.buffer.write_all(&0u16.to_be_bytes()).unwrap();

        if let Some((offset, more)) = fragment {
            // Needs to fit into 13 bits.
            assert!(offset < (1 << 13));
            let mut offset = offset as u16;

            // Fragment offsets are specified in multiples of 8.
            assert!(offset % 8 == 0);
            offset /= 8;

            // set more fragments bit if we expect there to be further packets.
            if more {
                offset |= 1 << 13;
            }

            // DF bit is set.
            self.buffer.write_all(&offset.to_be_bytes()).unwrap();
        } else {
            // DF bit is set.
            self.buffer.write_all(&[0x40, 0x0]).unwrap();
        }

        // TTL.
        self.buffer.write_all(&[200]).unwrap();

        // Protocol is UDP.
        self.buffer.write_all(&[17]).unwrap();

        // Omit the packet-level checksum.
        self.buffer.write_all(&0u16.to_be_bytes()).unwrap();

        // src, dst addresses
        self.buffer
            .write_all(&Ipv4Addr::LOCALHOST.octets())
            .unwrap();
        self.buffer
            .write_all(&Ipv4Addr::LOCALHOST.octets())
            .unwrap();
    }

    fn ip_fill_length(&mut self) {
        let start = self.ip_start.unwrap();
        let len_idx = self.ip_length_field.unwrap();

        let len = u16::try_from(self.buffer.len() - start)
            .unwrap()
            .to_be_bytes();
        self.buffer[len_idx..len_idx + 2].copy_from_slice(&len);
    }

    fn add_udp(&mut self) {
        self.udp_start = Some(self.buffer.len());

        // src, dst port
        self.buffer.write_all(&3433u16.to_be_bytes()).unwrap();
        self.buffer.write_all(&3433u16.to_be_bytes()).unwrap();

        self.udp_length_field = Some(self.buffer.len());

        // Length of UDP header + data.
        self.buffer.write_all(&0u16.to_be_bytes()).unwrap();

        // Skip checksum.
        self.buffer.write_all(&0u16.to_be_bytes()).unwrap();
    }

    fn add_dcquic(&mut self, packet_idx: u64) {
        // dcQUIC datagram.
        self.buffer.write_all(&[0x46]).unwrap();
        // Path secret ID
        self.buffer.write_all(&[0x43; 16]).unwrap();
        // Key ID
        encode_varint(&mut self.buffer, packet_idx);

        // Source control port.
        self.buffer.write_all(&3443u16.to_be_bytes()).unwrap();

        // Packet number.
        encode_varint(&mut self.buffer, 0);

        let payload_len = 110;

        // Payload length.
        encode_varint(&mut self.buffer, payload_len);

        for _ in 0..payload_len {
            self.buffer.push(0x55);
        }

        // Auth tag.
        self.buffer.write_all(&[1; 16]).unwrap();
    }

    fn udp_fill_length(&mut self) {
        let start = self.udp_start.unwrap();
        let len_idx = self.udp_length_field.unwrap();

        let len = u16::try_from(self.buffer.len() - start)
            .unwrap()
            .to_be_bytes();
        self.buffer[len_idx..len_idx + 2].copy_from_slice(&len);
    }
}

fn encode_varint(output: &mut Vec<u8>, input: u64) {
    let varint = s2n_quic_core::varint::VarInt::try_from(input).unwrap();
    let mut buffer = [0; 8];
    let mut buffer = EncoderBuffer::new(&mut buffer);
    varint.encode(&mut buffer);
    output.extend_from_slice(buffer.as_mut_slice());
}
