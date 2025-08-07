// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::Buffer, dissect, value::Parsed};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    buffer::{reader::Storage, Reader},
    packet::KeyPhase,
    stream::testing::Data,
    varint::VarInt,
};
use s2n_quic_dc::{
    credentials,
    packet::{self, stream, WireVersion},
};
use std::{collections::HashMap, num::NonZeroU16, ptr, time::Duration};

#[derive(Clone, Debug, bolero::TypeGenerator)]
struct StreamPacket {
    credentials: s2n_quic_dc::credentials::Credentials,
    source_queue_id: Option<VarInt>,
    stream_id: stream::Id,
    packet_space: stream::PacketSpace,
    packet_number: VarInt,
    next_expected_control_packet: VarInt,
    application_header: Data,
    payload: Data,
    key_phase: KeyPhase,
}

#[test]
fn check_stream_parse() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!()
        .with_type()
        .for_each(|packet: &StreamPacket| {
            let mut packet = packet.clone();
            let key = TestKey(packet.key_phase);
            let sent_payload = packet.payload;
            let sent_app_header_len = packet.application_header.buffered_len();
            let mut buffer = vec![
                0;
                sent_payload.buffered_len()
                    + sent_app_header_len
                    + s2n_quic_dc::packet::stream::encoder::MAX_HEADER_LEN
                    + s2n_quic_dc::packet::stream::encoder::MAX_RETRANSMISSION_HEADER_LEN
            ];
            let length = s2n_quic_dc::packet::stream::encoder::encode(
                EncoderBuffer::new(&mut buffer),
                packet.source_queue_id,
                packet.stream_id,
                packet.packet_number,
                packet.next_expected_control_packet,
                VarInt::new(packet.application_header.buffered_len() as u64).unwrap(),
                &mut packet.application_header,
                // FIXME: Consider populating control data? Not sent by current impl.
                VarInt::ZERO,
                &(),
                &mut packet.payload,
                &key,
                &packet.credentials,
            );

            let fields = crate::field::get();
            let mut tracker = Tracker::default();

            let mut buffer = unsafe { Buffer::new(ptr::null_mut(), &buffer[..length]) };
            let tag: Parsed<packet::stream::Tag> = buffer.consume().unwrap();
            assert!(dissect::stream(&mut tracker, fields, tag, &mut buffer, &mut ()).is_some());
            let tag: Parsed<u8> = tag.map(|v| v.into());

            assert_eq!(tracker.remove(fields.tag), Field::Integer(tag.value as u64));
            assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
            assert_eq!(
                tracker.remove(fields.path_secret_id),
                Field::Slice(packet.credentials.id.to_vec())
            );
            assert_eq!(
                tracker.remove(fields.key_id),
                Field::Integer(packet.credentials.key_id.into())
            );
            assert_eq!(
                tracker.take(fields.source_queue_id),
                packet.source_queue_id.map(|v| Field::Integer(v.as_u64()))
            );
            assert_eq!(
                tracker.remove(fields.queue_id),
                Field::Integer(u64::from(packet.stream_id.queue_id))
            );
            assert_eq!(
                tracker.remove(fields.is_reliable),
                Field::Integer(if packet.stream_id.is_reliable {
                    stream::id::IS_RELIABLE_MASK
                } else {
                    0
                })
            );
            assert_eq!(
                tracker.remove(fields.is_bidirectional),
                Field::Integer(if packet.stream_id.is_bidirectional {
                    stream::id::IS_BIDIRECTIONAL_MASK
                } else {
                    0
                })
            );
            assert_eq!(
                tracker.remove(fields.packet_number),
                Field::Integer(u64::from(packet.packet_number))
            );
            assert_eq!(
                tracker.remove(fields.next_expected_control_packet),
                Field::Integer(u64::from(packet.next_expected_control_packet))
            );

            // Tag fields all store the tag value itself.
            for field in [
                fields.stream_has_source_queue_id,
                fields.is_recovery_packet,
                fields.has_control_data,
                fields.has_final_offset,
                fields.has_application_header,
                fields.key_phase,
            ] {
                assert_eq!(tracker.remove(field), Field::Integer(tag.value as u64));
            }

            assert_eq!(
                tracker.remove(fields.payload_len),
                Field::Integer(sent_payload.buffered_len() as u64)
            );

            // FIXME: Deeper check?
            assert!(tracker.take(fields.payload).is_some());

            // FIXME: Deeper check?
            assert!(tracker.take(fields.auth_tag).is_some());

            assert_eq!(
                tracker.take(fields.final_offset),
                sent_payload
                    .final_offset()
                    .map(|v| Field::Integer(v.as_u64())),
            );

            assert_eq!(
                tracker.remove(fields.stream_offset),
                Field::Integer(sent_payload.current_offset().as_u64())
            );

            // FIXME: Figure out how to best test this - right now this is filled in as part of
            // *decoding* since we store the encrypted payloads.
            if packet.stream_id.is_reliable {
                assert_eq!(
                    tracker.remove(fields.relative_packet_number),
                    Field::Integer(0)
                );
            }

            if sent_app_header_len > 0 {
                assert_eq!(
                    tracker.remove(fields.application_header).slice_len(),
                    sent_app_header_len
                );
                assert_eq!(
                    tracker.remove(fields.application_header_len),
                    Field::Integer(sent_app_header_len as u64)
                );
            }

            assert_eq!(
                tracker.seen_fields.borrow().len(),
                0,
                "{:?}, remaining: {:?}",
                fields,
                tracker.seen_fields
            );
        });
}

#[derive(Clone, Debug, bolero::TypeGenerator)]
struct DatagramPacket {
    credentials: s2n_quic_dc::credentials::Credentials,
    source_control_port: NonZeroU16,
    // If None then not connected.
    packet_number: Option<VarInt>,
    next_expected_control_packet: Option<VarInt>,
    application_header: Data,
    payload: Data,
    key_phase: KeyPhase,
}

#[test]
fn check_datagram_parse() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!()
        .with_type()
        .for_each(|packet: &DatagramPacket| {
            let mut packet = packet.clone();
            if packet.next_expected_control_packet.is_some() && packet.packet_number.is_none() {
                packet.packet_number = Some(Default::default());
            }
            let key = TestKey(packet.key_phase);
            let sent_payload = packet.payload;
            let sent_app_header_len = packet.application_header.buffered_len();
            let mut buffer = vec![
                0;
                sent_payload.buffered_len()
                    + sent_app_header_len
                    + s2n_quic_dc::packet::stream::encoder::MAX_HEADER_LEN
                    + s2n_quic_dc::packet::stream::encoder::MAX_RETRANSMISSION_HEADER_LEN
            ];
            let length = s2n_quic_dc::packet::datagram::encoder::encode(
                EncoderBuffer::new(&mut buffer),
                packet.source_control_port.get(),
                packet.packet_number,
                packet.next_expected_control_packet,
                VarInt::new(packet.application_header.buffered_len() as u64).unwrap(),
                &mut packet.application_header,
                // FIXME: Encode real control data.
                &(),
                VarInt::new(packet.payload.buffered_len() as u64).unwrap(),
                &mut packet.payload,
                &key,
                &packet.credentials,
            );

            let fields = crate::field::get();
            let mut tracker = Tracker::default();

            let mut buffer = unsafe { Buffer::new(ptr::null_mut(), &buffer[..length]) };
            let tag: Parsed<packet::datagram::Tag> = buffer.consume().unwrap();
            assert!(dissect::datagram(&mut tracker, fields, tag, &mut buffer, &mut ()).is_some());
            let tag: Parsed<u8> = tag.map(|v| v.into());

            assert_eq!(tracker.remove(fields.tag), Field::Integer(tag.value as u64));
            assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
            assert_eq!(
                tracker.remove(fields.path_secret_id),
                Field::Slice(packet.credentials.id.to_vec())
            );
            assert_eq!(
                tracker.remove(fields.key_id),
                Field::Integer(packet.credentials.key_id.into())
            );
            assert_eq!(
                tracker.remove(fields.source_control_port),
                Field::Integer(packet.source_control_port.get() as u64)
            );
            assert_eq!(
                tracker.take(fields.packet_number),
                packet.packet_number.map(|v| Field::Integer(u64::from(v)))
            );
            assert_eq!(
                tracker.take(fields.next_expected_control_packet),
                packet
                    .next_expected_control_packet
                    .map(|v| Field::Integer(u64::from(v)))
            );

            // Tag fields all store the tag value itself.
            for field in [
                fields.is_ack_eliciting,
                fields.has_application_header,
                fields.is_connected,
                fields.key_phase,
            ] {
                assert_eq!(tracker.remove(field), Field::Integer(tag.value as u64));
            }

            assert_eq!(
                tracker.remove(fields.payload_len),
                Field::Integer(sent_payload.buffered_len() as u64)
            );

            // FIXME: Deeper check?
            assert!(tracker.take(fields.payload).is_some());

            // FIXME: Deeper check?
            assert!(tracker.take(fields.auth_tag).is_some());

            if sent_app_header_len > 0 {
                assert_eq!(
                    tracker.remove(fields.application_header).slice_len(),
                    sent_app_header_len
                );
                assert_eq!(
                    tracker.remove(fields.application_header_len),
                    Field::Integer(sent_app_header_len as u64)
                );
            }

            // Is ack-eliciting?
            //
            // For now no control data is encoded, but we should still find the fields.
            if packet.next_expected_control_packet.is_some() {
                assert_eq!(tracker.remove(fields.control_data).slice_len(), 0);
                assert_eq!(tracker.remove(fields.control_data_len), Field::Integer(0));
            }

            assert_eq!(
                tracker.seen_fields.borrow().len(),
                0,
                "{:?}, remaining: {:?}",
                fields,
                tracker.seen_fields
            );
        });
}

#[derive(Clone, Debug, bolero::TypeGenerator)]
struct ControlPacket {
    credentials: s2n_quic_dc::credentials::Credentials,
    source_queue_id: Option<VarInt>,
    stream_id: Option<stream::Id>,
    packet_number: VarInt,
    next_expected_control_packet: Option<VarInt>,
    application_header: Data,
    control_data: Data,
    key_phase: KeyPhase,
}

#[test]
fn check_control_parse() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!()
        .with_type()
        .for_each(|packet: &ControlPacket| {
            let mut packet = packet.clone();
            let key = TestKey(packet.key_phase);
            let sent_app_header_len = packet.application_header.buffered_len();
            let mut buffer = vec![
                0;
            packet.control_data.buffered_len()
                  +  sent_app_header_len
                    + s2n_quic_dc::packet::stream::encoder::MAX_HEADER_LEN
                    + s2n_quic_dc::packet::stream::encoder::MAX_RETRANSMISSION_HEADER_LEN
            ];
            let length = s2n_quic_dc::packet::control::encoder::encode(
                EncoderBuffer::new(&mut buffer),
                packet.source_queue_id,
                packet.stream_id,
                packet.packet_number,
                VarInt::new(packet.application_header.buffered_len() as u64).unwrap(),
                &mut packet.application_header,
                VarInt::new(packet.control_data.buffered_len() as u64).unwrap(),
                // FIXME: Encode *real* control data, not random garbage.
                &&packet.control_data.read_chunk(usize::MAX).unwrap()[..],
                &key,
                &packet.credentials,
            );

            let fields = crate::field::get();
            let mut tracker = Tracker::default();

            let mut buffer = unsafe { Buffer::new(ptr::null_mut(), &buffer[..length]) };
            let tag: Parsed<packet::control::Tag> = buffer.consume().unwrap();
            assert!(dissect::control(&mut tracker, fields, tag, &mut buffer, &mut ()).is_some());
            let tag: Parsed<u8> = tag.map(|v| v.into());

            // Tag fields all store the tag value itself.
            for field in [
                fields.control_has_source_queue_id,
                fields.is_stream,
                fields.has_application_header,
                fields.key_phase,
            ] {
                assert_eq!(tracker.remove(field), Field::Integer(tag.value as u64));
            }

            assert_eq!(tracker.remove(fields.tag), Field::Integer(tag.value as u64));
            assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
            assert_eq!(
                tracker.remove(fields.path_secret_id),
                Field::Slice(packet.credentials.id.to_vec())
            );
            assert_eq!(
                tracker.remove(fields.key_id),
                Field::Integer(packet.credentials.key_id.into())
            );
            if let Some(source_queue_id) = packet.source_queue_id {
                assert_eq!(
                    tracker.remove(fields.source_queue_id),
                    Field::Integer(source_queue_id.as_u64())
                );
            }
            assert_eq!(
                tracker.remove(fields.packet_number),
                Field::Integer(packet.packet_number.as_u64())
            );

            // FIXME: Deeper check?
            assert!(tracker.take(fields.auth_tag).is_some());

            if sent_app_header_len > 0 {
                assert_eq!(
                    tracker.remove(fields.application_header).slice_len(),
                    sent_app_header_len
                );
                assert_eq!(
                    tracker.remove(fields.application_header_len),
                    Field::Integer(sent_app_header_len as u64)
                );
            }

            // FIXME: In order to make any assertion here we'd need to not encode random data into
            // control packets. So skip it for now.
            // assert_eq!(
            //     tracker.seen_fields.borrow().len(),
            //     0,
            //     "{:?}, remaining: {:?}",
            //     fields,
            //     tracker.seen_fields
            // );
        });
}

#[derive(Clone, Debug, bolero::TypeGenerator)]
enum SecretControlPacket {
    UnknownPathSecret {
        id: credentials::Id,
        queue_id: Option<VarInt>,
        auth_tag: [u8; 16],
    },
    StaleKey {
        id: credentials::Id,
        key_id: VarInt,
        queue_id: Option<VarInt>,
    },
    ReplayDetected {
        id: credentials::Id,
        key_id: VarInt,
        queue_id: Option<VarInt>,
    },
}

#[test]
fn check_secret_control_parse() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!()
        .with_type()
        .for_each(|packet: &SecretControlPacket| {
            // Use a fixed key, we don't change key IDs per control packet anyway.
            let key = TestKey(KeyPhase::Zero);
            let mut buffer = vec![0; s2n_quic_dc::packet::secret_control::MAX_PACKET_SIZE];
            let length = match packet {
                SecretControlPacket::UnknownPathSecret {
                    id,
                    auth_tag,
                    queue_id,
                } => s2n_quic_dc::packet::secret_control::UnknownPathSecret {
                    wire_version: WireVersion::ZERO,
                    credential_id: *id,
                    queue_id: *queue_id,
                }
                .encode(EncoderBuffer::new(&mut buffer), auth_tag),
                SecretControlPacket::StaleKey {
                    id,
                    key_id,
                    queue_id,
                } => s2n_quic_dc::packet::secret_control::StaleKey {
                    wire_version: WireVersion::ZERO,
                    credential_id: *id,
                    queue_id: *queue_id,
                    min_key_id: *key_id,
                }
                .encode(EncoderBuffer::new(&mut buffer), &key),
                SecretControlPacket::ReplayDetected {
                    id,
                    key_id,
                    queue_id,
                } => s2n_quic_dc::packet::secret_control::ReplayDetected {
                    wire_version: WireVersion::ZERO,
                    credential_id: *id,
                    queue_id: *queue_id,
                    rejected_key_id: *key_id,
                }
                .encode(EncoderBuffer::new(&mut buffer), &key),
            };

            let fields = crate::field::get();
            let mut tracker = Tracker::default();

            let mut buffer = unsafe { Buffer::new(ptr::null_mut(), &buffer[..length]) };
            let tag = buffer.consume().unwrap();
            assert!(
                dissect::secret_control(&mut tracker, fields, tag, &mut buffer, &mut ()).is_some()
            );

            match packet {
                SecretControlPacket::UnknownPathSecret {
                    id,
                    auth_tag,
                    queue_id,
                } => {
                    if queue_id.is_some() {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0100));
                    } else {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0000));
                    }
                    assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
                    if let Some(queue_id) = queue_id {
                        assert_eq!(
                            tracker.remove(fields.queue_id),
                            Field::Integer(queue_id.as_u64())
                        );
                    }
                    assert_eq!(
                        tracker.remove(fields.path_secret_id),
                        Field::Slice(id.to_vec())
                    );
                    assert_eq!(
                        tracker.remove(fields.auth_tag),
                        Field::Slice(auth_tag.to_vec())
                    );
                }
                SecretControlPacket::StaleKey {
                    id,
                    key_id,
                    queue_id,
                } => {
                    if queue_id.is_some() {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0101));
                    } else {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0001));
                    }
                    assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
                    if let Some(queue_id) = queue_id {
                        assert_eq!(
                            tracker.remove(fields.queue_id),
                            Field::Integer(queue_id.as_u64())
                        );
                    }
                    assert_eq!(
                        tracker.remove(fields.path_secret_id),
                        Field::Slice(id.to_vec())
                    );
                    assert_eq!(
                        tracker.remove(fields.min_key_id),
                        Field::Integer(key_id.as_u64())
                    );
                    // FIXME: Deeper check?
                    assert!(tracker.take(fields.auth_tag).is_some());
                }
                SecretControlPacket::ReplayDetected {
                    id,
                    key_id,
                    queue_id,
                } => {
                    if queue_id.is_some() {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0110));
                    } else {
                        assert_eq!(tracker.remove(fields.tag), Field::Integer(0b0110_0010));
                    }
                    assert_eq!(tracker.remove(fields.wire_version), Field::Integer(0));
                    if let Some(queue_id) = queue_id {
                        assert_eq!(
                            tracker.remove(fields.queue_id),
                            Field::Integer(queue_id.as_u64())
                        );
                    }
                    assert_eq!(
                        tracker.remove(fields.path_secret_id),
                        Field::Slice(id.to_vec())
                    );
                    assert_eq!(
                        tracker.remove(fields.rejected_key_id),
                        Field::Integer(key_id.as_u64())
                    );
                    // FIXME: Deeper check?
                    assert!(tracker.take(fields.auth_tag).is_some());
                }
            };

            assert_eq!(
                tracker.seen_fields.borrow().len(),
                0,
                "{:?}, remaining: {:?}",
                fields,
                tracker.seen_fields
            );
        });
}

#[test]
fn random_stream_packets() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!().for_each(|packet: &[u8]| {
        let fields = crate::field::get();
        let mut tracker = Tracker::default();
        let mut buffer = unsafe { Buffer::new(ptr::null_mut(), packet) };
        let Some(tag) = buffer.consume() else {
            return;
        };
        // May fail to parse, but shouldn't panic.
        let _ = dissect::stream(&mut tracker, fields, tag, &mut buffer, &mut ());
    });
}

#[test]
fn random_segments() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!().for_each(|packet: &[u8]| {
        let fields = crate::field::get();
        let mut tracker = Tracker::default();
        let mut buffer = unsafe { Buffer::new(ptr::null_mut(), packet) };
        let Some(tag) = buffer.consume() else {
            return;
        };
        // May fail to parse, but shouldn't panic.
        let _ = dissect::segment(
            &mut tracker,
            &mut (),
            fields,
            tag,
            &mut buffer,
            &mut (),
            dissect::Protocol::Udp,
        );
    });
}

#[test]
fn random_datagram_packets() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!().for_each(|packet: &[u8]| {
        let fields = crate::field::get();
        let mut tracker = Tracker::default();
        let mut buffer = unsafe { Buffer::new(ptr::null_mut(), packet) };
        let Some(tag) = buffer.consume() else {
            return;
        };
        // May fail to parse, but shouldn't panic.
        let _ = dissect::datagram(&mut tracker, fields, tag, &mut buffer, &mut ());
    });
}

#[test]
fn random_control_packets() {
    // Initialize field IDs.
    let _ = crate::field::get();

    bolero::check!().for_each(|packet: &[u8]| {
        let fields = crate::field::get();
        let mut tracker = Tracker::default();
        let mut buffer = unsafe { Buffer::new(ptr::null_mut(), packet) };
        let Some(tag) = buffer.consume() else {
            return;
        };
        // May fail to parse, but shouldn't panic.
        let _ = dissect::control(&mut tracker, fields, tag, &mut buffer, &mut ());
    });
}

#[derive(Default, Clone)]
struct Tracker {
    seen_fields: std::rc::Rc<std::cell::RefCell<HashMap<i32, Field>>>,
}

impl Tracker {
    fn put(&mut self, id: i32, field: Field) {
        assert!(self.seen_fields.borrow_mut().insert(id, field).is_none());
    }

    #[track_caller]
    fn remove(&mut self, id: i32) -> Field {
        self.seen_fields.borrow_mut().remove(&id).unwrap()
    }

    fn take(&mut self, id: i32) -> Option<Field> {
        self.seen_fields.borrow_mut().remove(&id)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Field {
    Integer(u64),
    Slice(Vec<u8>),
    Duration(Duration),
}

impl Field {
    #[track_caller]
    fn slice_len(&self) -> usize {
        match self {
            Field::Slice(s) => s.len(),
            Field::Integer(_) => panic!("expecting slice found integer"),
            Field::Duration(_) => panic!("expecting slice found duration"),
        }
    }
}

impl crate::wireshark::Node for Tracker {
    type AddedItem = ();

    fn add_slice(
        &mut self,
        _buffer: &Buffer,
        field: i32,
        parsed: Parsed<&[u8]>,
    ) -> Self::AddedItem {
        self.put(field, Field::Slice(parsed.value.to_vec()));
    }

    fn add_slice_hidden(
        &mut self,
        _buffer: &Buffer,
        field: i32,
        parsed: Parsed<&[u8]>,
    ) -> Self::AddedItem {
        self.put(field, Field::Slice(parsed.value.to_vec()));
    }

    fn add_u64(&mut self, _buffer: &Buffer, field: i32, parsed: Parsed<u64>) -> Self::AddedItem {
        self.put(field, Field::Integer(parsed.value));
    }

    fn add_u32(&mut self, _buffer: &Buffer, field: i32, parsed: Parsed<u32>) -> Self::AddedItem {
        self.put(field, Field::Integer(parsed.value as u64));
    }

    fn add_u16(&mut self, _buffer: &Buffer, field: i32, parsed: Parsed<u16>) -> Self::AddedItem {
        self.put(field, Field::Integer(parsed.value as u64));
    }

    fn add_u8(&mut self, _buffer: &Buffer, field: i32, parsed: Parsed<u8>) -> Self::AddedItem {
        self.put(field, Field::Integer(parsed.value as u64));
    }

    fn add_boolean<T: Into<u8>>(
        &mut self,
        _buffer: &Buffer,
        field: i32,
        parsed: Parsed<T>,
    ) -> Self::AddedItem {
        self.put(field, Field::Integer(parsed.value.into() as u64));
    }

    fn add_duration(
        &mut self,
        _buffer: &Buffer,
        field: i32,
        parsed: Parsed<Duration>,
    ) -> Self::AddedItem {
        self.put(field, Field::Duration(parsed.value));
    }

    fn add_subtree(&mut self, _: Self::AddedItem, _: i32) -> Self {
        self.clone()
    }
}

impl crate::wireshark::Item for () {
    fn append_text(&mut self, _: &'static std::ffi::CStr) {
        // no-op
    }
}

struct TestKey(KeyPhase);

impl s2n_quic_dc::crypto::seal::Application for TestKey {
    fn key_phase(&self) -> KeyPhase {
        self.0
    }

    fn tag_len(&self) -> usize {
        16
    }

    fn encrypt(
        &self,
        _packet_number: u64,
        _header: &[u8],
        extra_payload: Option<&[u8]>,
        payload_and_tag: &mut [u8],
    ) {
        if let Some(extra_payload) = extra_payload {
            let offset = payload_and_tag.len() - self.tag_len() - extra_payload.len();
            let dest = &mut payload_and_tag[offset..];
            assert!(dest.len() == extra_payload.len() + self.tag_len());
            let (dest, tag) = dest.split_at_mut(extra_payload.len());
            dest.copy_from_slice(extra_payload);
            tag.fill(0);
        }
    }
}

impl s2n_quic_dc::crypto::seal::Control for TestKey {
    fn tag_len(&self) -> usize {
        16
    }

    fn sign(&self, _header: &[u8], tag: &mut [u8]) {
        tag.fill(0)
    }
}

impl s2n_quic_dc::crypto::seal::control::Stream for TestKey {
    fn retransmission_tag(
        &self,
        _original_packet_number: u64,
        _retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        tag_out.fill(0)
    }
}

impl s2n_quic_dc::crypto::seal::control::Secret for TestKey {}
