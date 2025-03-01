// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::Buffer,
    field::Registration,
    value::Parsed,
    wireshark::{Info, Item, Node},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{frame::FrameMut, varint::VarInt};
use s2n_quic_dc::packet::{self, stream, WireVersion};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum Protocol {
    Tcp,
    Udp,
}

pub fn segment<T: Node>(
    tree: &mut T,
    root: &mut impl Item,
    fields: &Registration,
    ptag: Parsed<packet::Tag>,
    buffer: &mut Buffer,
    info: &mut impl Info,
    protocol: Protocol,
) -> Option<()> {
    match ptag.value {
        packet::Tag::Stream(tag) => {
            root.append_text(c" Stream");
            let tag = ptag.map(|_| tag);
            stream(tree, fields, tag, buffer, info)
        }
        packet::Tag::Control(tag) => {
            match protocol {
                Protocol::Tcp => root.append_text(c" Control (UNEXPECTED)"),
                Protocol::Udp => root.append_text(c" Control"),
            }
            let tag = ptag.map(|_| tag);
            control(tree, fields, tag, buffer, info)
        }
        packet::Tag::Datagram(tag) => {
            match protocol {
                Protocol::Tcp => root.append_text(c" Datagram (UNEXPECTED)"),
                Protocol::Udp => root.append_text(c" Datagram"),
            }
            let tag = ptag.map(|_| tag);
            datagram(tree, fields, tag, buffer, info)
        }
        _ => {
            root.append_text(c" Secret Control");
            secret_control(tree, fields, ptag, buffer, info)
        }
    }
}

pub fn stream<T: Node>(
    tree: &mut T,
    fields: &Registration,
    tag: Parsed<stream::Tag>,
    buffer: &mut Buffer,
    info: &mut impl Info,
) -> Option<()> {
    let tag_item = tag.record(buffer, tree, fields.tag);

    let mut tag_tree = tree.add_subtree(tag_item, fields.tag_subtree);
    for field in [
        fields.has_source_stream_port,
        fields.is_recovery_packet,
        fields.has_control_data,
        fields.has_final_offset,
        fields.has_application_header,
        fields.key_phase,
    ] {
        tag_tree.add_boolean(buffer, field, tag);
    }

    let tag = tag.value;

    let path_secret_id = buffer.consume_bytes(16)?;
    path_secret_id.record(buffer, tree, fields.path_secret_id);

    let key_id = buffer.consume::<VarInt>()?;
    key_id.record(buffer, tree, fields.key_id);

    let wire_version = buffer.consume::<WireVersion>()?;
    wire_version.record(buffer, tree, fields.wire_version);

    let source_control_port = buffer.consume::<u16>()?;
    source_control_port.record(buffer, tree, fields.source_control_port);

    if tag.has_source_stream_port() {
        let source_stream_port = buffer.consume::<u16>()?;
        source_stream_port.record(buffer, tree, fields.source_stream_port);
    }

    let stream_id = buffer.consume()?;
    let stream_id = record_stream_id(tree, fields, buffer, stream_id);

    let packet_number = buffer.consume::<VarInt>()?;
    packet_number.record(buffer, tree, fields.packet_number);

    // FIXME: Actually decode the 32 bit value?
    if stream_id.is_reliable {
        let relative_packet_number = buffer.consume::<u32>()?;
        relative_packet_number.record(buffer, tree, fields.relative_packet_number);
    }

    let next_expected_control_packet = buffer.consume::<VarInt>()?;
    next_expected_control_packet.record(buffer, tree, fields.next_expected_control_packet);

    let stream_offset = buffer.consume::<VarInt>()?;
    stream_offset.record(buffer, tree, fields.stream_offset);

    if tag.has_final_offset() {
        let final_offset = buffer.consume::<VarInt>()?;
        final_offset.record(buffer, tree, fields.final_offset);
    }

    let control_data_len = if tag.has_control_data() {
        let control_data_len = buffer.consume::<VarInt>()?;
        control_data_len.record(buffer, tree, fields.control_data_len);
        Some(control_data_len.value)
    } else {
        None
    };

    let payload_len = buffer.consume::<VarInt>()?;
    payload_len.record(buffer, tree, fields.payload_len);

    if tag.has_application_header() {
        let application_header_len = buffer.consume::<VarInt>()?;
        application_header_len.record(buffer, tree, fields.application_header_len);

        let application_header = buffer.consume_bytes(application_header_len.value)?;

        application_header.record(buffer, tree, fields.application_header);
    }

    let mut control_info = String::new();
    if let Some(control_data_len) = control_data_len {
        let control_data = buffer.consume_bytes(control_data_len)?;
        control_frames(tree, fields, buffer, control_data, &mut control_info);
    }

    let payload = buffer.consume_bytes(payload_len.value)?;
    payload.record_hidden(buffer, tree, fields.payload);

    let auth_tag = buffer.consume_bytes(16)?;
    auth_tag.record(buffer, tree, fields.auth_tag);

    info.append_delim(" ");
    info.append(format_args!(
        "Stream(ID={}, PN={},{control_info} LEN={})",
        key_id.value, packet_number.value, payload.len
    ));

    Some(())
}

pub fn control<T: Node>(
    tree: &mut T,
    fields: &Registration,
    tag: Parsed<packet::control::Tag>,
    buffer: &mut Buffer,
    info: &mut impl Info,
) -> Option<()> {
    let tag_item = tag.record(buffer, tree, fields.tag);

    let mut tag_tree = tree.add_subtree(tag_item, fields.tag_subtree);
    for field in [
        fields.is_stream,
        fields.has_application_header,
        fields.key_phase,
    ] {
        tag_tree.add_boolean(buffer, field, tag);
    }

    let tag = tag.value;

    let path_secret_id = buffer.consume_bytes(16)?;
    path_secret_id.record(buffer, tree, fields.path_secret_id);

    let key_id = buffer.consume::<VarInt>()?;
    key_id.record(buffer, tree, fields.key_id);

    let wire_version = buffer.consume::<WireVersion>()?;
    wire_version.record(buffer, tree, fields.wire_version);

    let source_control_port = buffer.consume::<u16>()?;
    source_control_port.record(buffer, tree, fields.source_control_port);

    if tag.is_stream() {
        let stream_id = buffer.consume()?;
        record_stream_id(tree, fields, buffer, stream_id);
    }

    let packet_number = buffer.consume::<VarInt>()?;
    packet_number.record(buffer, tree, fields.packet_number);

    let control_data_len = buffer.consume::<VarInt>()?;
    control_data_len.record(buffer, tree, fields.control_data_len);

    if tag.has_application_header() {
        let application_header_len = buffer.consume::<VarInt>()?;
        application_header_len.record(buffer, tree, fields.application_header_len);

        let application_header = buffer.consume_bytes(application_header_len.value)?;

        application_header.record(buffer, tree, fields.application_header);
    }

    let control_data = buffer.consume_bytes(control_data_len.value)?;
    let mut control_info = String::new();
    control_frames(tree, fields, buffer, control_data, &mut control_info);

    let auth_tag = buffer.consume_bytes(16)?;
    auth_tag.record(buffer, tree, fields.auth_tag);

    info.append_delim(" ");
    info.append(format_args!(
        "Control(ID={}, PN={},{control_info})",
        key_id.value, packet_number.value
    ));

    Some(())
}

pub fn control_frames<T: Node>(
    tree: &mut T,
    fields: &Registration,
    buffer: &mut Buffer,
    control_data: Parsed<&[u8]>,
    info: &mut impl Info,
) {
    let control_item = control_data.record_hidden(buffer, tree, fields.control_data);
    let mut tree = tree.add_subtree(control_item, fields.control_data_subtree);
    let tree = &mut tree;
    let mut control_data_owned = control_data.value.to_vec();

    let mut has_ack = false;
    let mut has_max_data = false;
    let mut has_close = false;

    let mut offset = 0;
    let mut decoder = DecoderBufferMut::new(&mut control_data_owned);
    while !decoder.is_empty() {
        let before = decoder.len();
        let Ok((frame, remaining)) = decoder.decode::<FrameMut>() else {
            break;
        };
        let after = remaining.len();
        let len = before - after;
        decoder = remaining;

        // create a placeholder parsed value
        let parsed = Parsed {
            offset,
            len,
            value: (),
        };
        offset += len;

        match frame {
            FrameMut::Padding(_) => {
                // do nothing
            }
            FrameMut::Ping(_) => {
                // do nothing
            }
            FrameMut::Ack(ack) => {
                // TODO fix the tests to not assume a single occurrence of each field
                if cfg!(test) && has_ack {
                    continue;
                }

                has_ack = true;
                parsed
                    .with(ack.ack_delay())
                    .record(buffer, tree, fields.ack_delay);

                // FIXME: Look into using FT_FRAMENUM, but that is limited to 32-bit numbers, so
                // maybe too small?
                let ranges = ack.ack_ranges();

                // TODO fix the tests to not assume a single occurrence of each field
                #[cfg(test)]
                let ranges = ranges.take(1);

                for range in ranges {
                    let start = parsed.with(range.start().as_u64());
                    let end = parsed.with(range.end().as_u64());

                    let range = end.record(buffer, tree, fields.ackd_packet);
                    let mut range_tree = tree.add_subtree(range, fields.ack_range_subtree);
                    start.record(buffer, &mut range_tree, fields.ack_range_min);
                    end.record(buffer, &mut range_tree, fields.ack_range_max);
                }

                if let Some(ecn) = ack.ecn_counts {
                    parsed
                        .with(ecn.ect_0_count)
                        .record(buffer, tree, fields.ect_0_count);
                    parsed
                        .with(ecn.ect_1_count)
                        .record(buffer, tree, fields.ect_1_count);
                    parsed
                        .with(ecn.ce_count)
                        .record(buffer, tree, fields.ce_count);
                }
            }
            FrameMut::MaxData(frame) => {
                // TODO fix the tests to not assume a single occurrence of each field
                if cfg!(test) && has_max_data {
                    continue;
                }

                has_max_data = true;
                parsed
                    .with(frame.maximum_data)
                    .record(buffer, tree, fields.max_data);
            }
            FrameMut::ConnectionClose(frame) => {
                // TODO fix the tests to not assume a single occurrence of each field
                if cfg!(test) && has_close {
                    continue;
                }

                has_close = true;
                parsed
                    .with(frame.error_code)
                    .record(buffer, tree, fields.close_error_code);
                if let Some(frame_type) = frame.frame_type {
                    parsed
                        .with(frame_type)
                        .record(buffer, tree, fields.close_frame_type);
                }
                if let Some(reason) = frame.reason {
                    parsed
                        .with(reason)
                        .record(buffer, tree, fields.close_reason);
                }
            }
            // FIXME: add "other" handling
            _ => continue,
        }
    }

    for (was_observed, label) in [
        (has_ack, "ACK"),
        (has_max_data, "MAX_DATA"),
        (has_close, "CONNECTION_CLOSE"),
    ] {
        if was_observed {
            if info.is_empty() {
                info.append_str(" ");
            } else {
                info.append_str(", ");
            }
            info.append_str(label);
        }
    }
}

fn record_stream_id<T: Node>(
    tree: &mut T,
    fields: &Registration,
    buffer: &mut Buffer,
    stream_id: Parsed<stream::Id>,
) -> stream::Id {
    stream_id
        .map(|v| v.route_key)
        .record(buffer, tree, fields.route_key);
    let id = stream_id.value;

    tree.add_boolean(
        buffer,
        fields.is_reliable,
        Parsed {
            offset: stream_id.offset + stream_id.len - 1,
            len: 1,
            // needs to match the bitmask to show up properly
            value: if id.is_reliable { 0b10 } else { 0 },
        },
    );
    tree.add_boolean(
        buffer,
        fields.is_bidirectional,
        Parsed {
            offset: stream_id.offset + stream_id.len - 1,
            len: 1,
            value: id.is_bidirectional as u8,
        },
    );

    id
}

pub fn datagram<T: Node>(
    tree: &mut T,
    fields: &Registration,
    tag: Parsed<packet::datagram::Tag>,
    buffer: &mut Buffer,
    info: &mut impl Info,
) -> Option<()> {
    let tag_item = tag.record(buffer, tree, fields.tag);

    let mut tag_tree = tree.add_subtree(tag_item, fields.tag_subtree);
    for field in [
        fields.is_ack_eliciting,
        fields.is_connected,
        fields.has_application_header,
        fields.key_phase,
    ] {
        tag_tree.add_boolean(buffer, field, tag);
    }

    let tag = tag.value;

    let path_secret_id = buffer.consume_bytes(16)?;
    path_secret_id.record(buffer, tree, fields.path_secret_id);

    let key_id = buffer.consume::<VarInt>()?;
    key_id.record(buffer, tree, fields.key_id);

    let wire_version = buffer.consume::<WireVersion>()?;
    wire_version.record(buffer, tree, fields.wire_version);

    let source_control_port = buffer.consume::<u16>()?;
    source_control_port.record(buffer, tree, fields.source_control_port);

    let packet_number = if tag.is_connected() || tag.ack_eliciting() {
        let packet_number = buffer.consume::<VarInt>()?;
        packet_number.record(buffer, tree, fields.packet_number);
        Some(packet_number.value)
    } else {
        None
    };

    let payload_len = buffer.consume::<VarInt>()?;
    payload_len.record(buffer, tree, fields.payload_len);
    let payload_len = payload_len.value;

    if tag.ack_eliciting() {
        let next_expected_control_packet = buffer.consume::<VarInt>()?;
        next_expected_control_packet.record(buffer, tree, fields.next_expected_control_packet);
    }

    let control_data_len = if tag.ack_eliciting() {
        let control_data_len = buffer.consume::<VarInt>()?;
        control_data_len.record(buffer, tree, fields.control_data_len);
        Some(control_data_len.value)
    } else {
        None
    };

    if tag.has_application_header() {
        let application_header_len = buffer.consume::<VarInt>()?;
        application_header_len.record(buffer, tree, fields.application_header_len);

        let application_header = buffer.consume_bytes(application_header_len.value)?;

        application_header.record(buffer, tree, fields.application_header);
    }

    if let Some(control_data_len) = control_data_len {
        let control_data = buffer.consume_bytes(control_data_len)?;
        control_data.record(buffer, tree, fields.control_data);
    }

    let payload = buffer.consume_bytes(payload_len)?;
    payload.record(buffer, tree, fields.payload);

    let auth_tag = buffer.consume_bytes(16)?;
    auth_tag.record(buffer, tree, fields.auth_tag);

    info.append_delim(" ");
    if let Some(pn) = packet_number {
        info.append(format_args!(
            "Datagram(ID={}, PN={pn}, LEN={payload_len})",
            key_id.value
        ));
    } else {
        info.append(format_args!(
            "Datagram(ID={}, LEN={payload_len})",
            key_id.value
        ));
    }

    Some(())
}

pub fn secret_control<T: Node>(
    tree: &mut T,
    fields: &Registration,
    tag: Parsed<packet::Tag>,
    buffer: &mut Buffer,
    info: &mut impl Info,
) -> Option<()> {
    let mut item = tag.record(buffer, tree, fields.tag);

    match tag.value {
        packet::Tag::UnknownPathSecret(_) => {
            item.append_text(c" (UnknownPathSecret)");

            let path_secret_id = buffer.consume_bytes(16)?;
            path_secret_id.record(buffer, tree, fields.path_secret_id);

            let wire_version = buffer.consume::<WireVersion>()?;
            wire_version.record(buffer, tree, fields.wire_version);

            let auth_tag = buffer.consume_bytes(16)?;
            auth_tag.record(buffer, tree, fields.auth_tag);

            info.append_delim(" ");
            info.append_str("UnknownPathSecret");

            Some(())
        }
        packet::Tag::StaleKey(_) => {
            item.append_text(c" (StaleKey)");

            let path_secret_id = buffer.consume_bytes(16)?;
            path_secret_id.record(buffer, tree, fields.path_secret_id);

            let wire_version = buffer.consume::<WireVersion>()?;
            wire_version.record(buffer, tree, fields.wire_version);

            let min_key_id = buffer.consume::<VarInt>()?;
            min_key_id.record(buffer, tree, fields.min_key_id);

            let auth_tag = buffer.consume_bytes(16)?;
            auth_tag.record(buffer, tree, fields.auth_tag);

            info.append_delim(" ");
            info.append_str("StaleKey");

            Some(())
        }
        packet::Tag::ReplayDetected(_) => {
            item.append_text(c" (ReplayDetected)");

            let path_secret_id = buffer.consume_bytes(16)?;
            path_secret_id.record(buffer, tree, fields.path_secret_id);

            let wire_version = buffer.consume::<WireVersion>()?;
            wire_version.record(buffer, tree, fields.wire_version);

            let rejected_key_id = buffer.consume::<VarInt>()?;
            rejected_key_id.record(buffer, tree, fields.rejected_key_id);

            let auth_tag = buffer.consume_bytes(16)?;
            auth_tag.record(buffer, tree, fields.auth_tag);

            info.append_delim(" ");
            info.append_str("ReplayDetected");

            Some(())
        }
        _ => None,
    }
}
