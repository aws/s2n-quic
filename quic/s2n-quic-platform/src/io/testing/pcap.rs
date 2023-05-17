// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::network::{Packet, Recorder};
use pcap_file::{
    pcapng::{
        blocks::{
            enhanced_packet::EnhancedPacketBlock, interface_description::InterfaceDescriptionBlock,
        },
        PcapNgWriter,
    },
    DataLink,
};
use s2n_quic_core::{havoc::EncoderBuffer, inet::ExplicitCongestionNotification, io::tx, xdp};
use std::{borrow::Cow, collections::BTreeMap, fs, io, path::Path, sync::Mutex};

pub struct File {
    file: Mutex<State>,
}

impl File {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = State::new(path)?;
        let file = Mutex::new(file);
        let file = File { file };
        Ok(file)
    }
}

struct State {
    pcap: PcapNgWriter<fs::File>,
    interfaces: BTreeMap<u16, u32>,
    encoder: xdp::encoder::State,
}

impl State {
    fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = fs::File::create(path)?;
        let pcap = PcapNgWriter::new(file).unwrap();
        Ok(Self {
            pcap,
            interfaces: Default::default(),
            encoder: Default::default(),
        })
    }

    fn record(&mut self, packet: &Packet) {
        let interface_id = self.interface(packet.path.local_address.port());
        let timestamp = unsafe { super::time::now().as_duration() };
        let options = vec![];

        let mut payload = vec![0u8; packet.payload.len() + 100];
        let mut buffer = EncoderBuffer::new(&mut payload);

        let mut message = RawPacket {
            path: packet.path.into(),
            inner: packet,
        };

        // make the packets consistent between tx and rx
        self.encoder.set_ipv4_id(packet.id as _);

        s2n_quic_core::xdp::encoder::encode_packet(&mut buffer, &mut message, &mut self.encoder)
            .unwrap();

        let (payload, _) = buffer.split_off();

        let original_len = payload.len() as _;
        let data = Cow::Borrowed(payload);

        let block = EnhancedPacketBlock {
            interface_id,
            timestamp,
            original_len,
            data,
            options,
        };

        let _ = self.pcap.write_pcapng_block(block).unwrap();
    }

    fn interface(&mut self, port: u16) -> u32 {
        let next_id = self.interfaces.len() as u32;
        *self.interfaces.entry(port).or_insert_with(|| {
            let block = InterfaceDescriptionBlock {
                linktype: DataLink::ETHERNET,
                snaplen: 0,
                options: vec![],
            };

            self.pcap.write_pcapng_block(block).unwrap();
            next_id
        })
    }
}

impl Recorder for File {
    fn record(&self, packet: &Packet) {
        if let Ok(mut state) = self.file.lock() {
            state.record(packet);
        }
    }
}

struct RawPacket<'a> {
    path: s2n_quic_core::xdp::path::Tuple,
    inner: &'a Packet,
}

impl<'a> tx::Message for RawPacket<'a> {
    type Handle = s2n_quic_core::xdp::path::Tuple;

    fn path_handle(&self) -> &Self::Handle {
        &self.path
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.inner.ecn
    }

    fn delay(&mut self) -> core::time::Duration {
        todo!()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn can_gso(&self, _: usize, _: usize) -> bool {
        todo!()
    }

    fn write_payload(
        &mut self,
        mut buffer: tx::PayloadBuffer,
        _: usize,
    ) -> Result<usize, tx::Error> {
        buffer.write(&self.inner.payload)
    }
}
