// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::wireshark_sys;
use core::{ffi::CStr, ptr};
use std::sync::OnceLock;

static REGISTRATION: OnceLock<Registration> = OnceLock::new();

#[allow(dead_code)]
pub unsafe extern "C" fn proto_register() {
    let _ = get();
}

#[derive(Debug)]
pub struct Registration {
    pub protocol: i32,
    pub all_subtree: i32,
    pub tag_subtree: i32,
    pub control_data_subtree: i32,

    pub ack_range_subtree: i32,
    pub ack_range_min: i32,
    pub ack_range_max: i32,

    pub tag: i32,
    pub is_ack_eliciting: i32,
    pub is_connected: i32,
    pub has_application_header: i32,
    pub has_source_stream_port: i32,
    pub is_recovery_packet: i32,
    pub has_control_data: i32,
    pub has_final_offset: i32,
    pub key_phase: i32,
    pub wire_version: i32,
    pub path_secret_id: i32,
    pub key_id: i32,
    pub source_control_port: i32,
    pub source_stream_port: i32,
    pub packet_number: i32,
    pub payload_len: i32,
    pub next_expected_control_packet: i32,
    pub control_data_len: i32,
    pub application_header_len: i32,
    pub application_header: i32,
    pub control_data: i32,
    pub payload: i32,
    pub auth_tag: i32,

    pub is_bidirectional: i32,
    pub is_reliable: i32,
    pub stream_id: i32,
    pub relative_packet_number: i32,
    pub stream_offset: i32,
    pub final_offset: i32,

    pub is_stream: i32,
    pub ack_delay: i32,
    pub ackd_packet: i32,
    pub ect_0_count: i32,
    pub ect_1_count: i32,
    pub ce_count: i32,
    pub max_data: i32,

    pub close_error_code: i32,
    pub close_frame_type: i32,
    pub close_reason: i32,

    pub min_key_id: i32,
    pub rejected_key_id: i32,
}

#[cfg_attr(test, allow(unused))]
fn register_field(protocol: Protocol, field: Field) -> i32 {
    let protocol = protocol.0;
    let hfinfo = field.info;

    #[cfg(test)]
    static FIELD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    #[cfg(not(test))]
    unsafe {
        let id = Box::leak(Box::new(0i32));
        let registration_array = Box::into_raw(Box::new(wireshark_sys::hf_register_info {
            p_id: id as *mut _,
            hfinfo,
        }));
        crate::wireshark_sys::proto_register_field_array(protocol, registration_array, 1);
        *id
    }

    #[cfg(test)]
    {
        FIELD_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as i32
    }
}

#[must_use]
struct Field {
    protocol: Protocol,
    info: wireshark_sys::_header_field_info,
}

impl Field {
    fn with_mask(mut self, mask: u64) -> Self {
        self.info.bitmask = mask;
        self
    }

    fn register(self) -> i32 {
        register_field(self.protocol, self)
    }
}

fn register_subtree() -> i32 {
    let id = Box::leak(Box::new(-1i32));
    #[cfg(not(test))]
    unsafe {
        crate::wireshark_sys::proto_register_subtree_array(&(id as *mut i32), 1);
    }
    *id
}

#[derive(Clone, Copy)]
struct Protocol(i32);

impl Protocol {
    fn field(
        self,
        name: &'static CStr,
        abbrev: &'static CStr,
        type_: wireshark_sys::ftenum_t,
        display: wireshark_sys::field_display_e,
        blurb: &'static CStr,
    ) -> Field {
        Field {
            protocol: self,
            info: wireshark_sys::_header_field_info {
                name: name.as_ptr(),
                abbrev: abbrev.as_ptr(),
                type_,
                display: display as _,
                strings: ptr::null(),
                bitmask: 0,
                blurb: blurb.as_ptr(),

                // Following fields are filled/used by Wireshark internally.

                // -1, 0, HF_REF_TYPE_NONE, -1, NULL
                id: -1,
                parent: 0,
                ref_type: wireshark_sys::hf_ref_type_HF_REF_TYPE_NONE,
                same_name_prev_id: -1,
                same_name_next: ptr::null_mut(),
            },
        }
    }
}

#[cfg(test)]
fn register_protocol() -> Protocol {
    Protocol(10101)
}

#[cfg(not(test))]
fn register_protocol() -> Protocol {
    let name = concat!(env!("PLUGIN_NAME"), "\0");
    let lower = concat!(env!("PLUGIN_NAME_LOWER"), "\0");
    Protocol(unsafe {
        wireshark_sys::proto_register_protocol(
            name.as_ptr() as *const _,
            name.as_ptr() as *const _,
            lower.as_ptr() as *const _,
        )
    })
}

pub fn get() -> &'static Registration {
    REGISTRATION.get_or_init(init)
}

fn init() -> Registration {
    use wireshark_sys::{
        field_display_e_BASE_DEC as BASE_DEC, field_display_e_BASE_HEX as BASE_HEX,
        field_display_e_BASE_NONE as BASE_NONE, field_display_e_SEP_DOT as SEP_DOT,
        ftenum_FT_BOOLEAN as BOOLEAN, ftenum_FT_BYTES as BYTES,
        ftenum_FT_RELATIVE_TIME as RELATIVE_TIME, ftenum_FT_STRING as STRING,
        ftenum_FT_UINT16 as UINT16, ftenum_FT_UINT32 as UINT32, ftenum_FT_UINT64 as UINT64,
        ftenum_FT_UINT8 as UINT8,
    };

    let protocol = register_protocol();

    Registration {
        protocol: protocol.0,
        all_subtree: register_subtree(),
        tag_subtree: register_subtree(),
        control_data_subtree: register_subtree(),
        ack_range_subtree: register_subtree(),
        tag: protocol
            .field(c"Tag", c"dcquic.tag", UINT8, BASE_HEX, c"dcQUIC packet tag")
            .register(),
        is_ack_eliciting: protocol
            .field(
                c"Is ack eliciting?",
                c"dcquic.tag.is_ack_eliciting",
                BOOLEAN,
                SEP_DOT,
                c"Will packet elicit acknowledgements?",
            )
            .with_mask(masks::ACK_ELICITING)
            .register(),
        is_connected: protocol
            .field(
                c"Is connected?",
                c"dcquic.tag.is_connected",
                BOOLEAN,
                SEP_DOT,
                c"Is the application using a connected dcQUIC client?",
            )
            .with_mask(masks::IS_CONNECTED)
            .register(),
        has_application_header: protocol
            .field(
                c"Has application header?",
                c"dcquic.tag.has_application_header",
                BOOLEAN,
                SEP_DOT,
                c"Does the packet contain an authenticated plaintext application-provided header?",
            )
            .with_mask(masks::HAS_APPLICATION_HEADER)
            .register(),
        has_source_stream_port: protocol
            .field(
                c"Has source stream port?",
                c"dcquic.tag.has_source_stream_port",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(masks::HAS_SOURCE_STREAM_PORT)
            .register(),
        is_recovery_packet: protocol
            .field(
                c"Is recovery packet?",
                c"dcquic.tag.is_recovery_packet",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(masks::IS_RECOVERY_PACKET)
            .register(),
        has_control_data: protocol
            .field(
                c"Has control data?",
                c"dcquic.tag.has_control_data",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(masks::HAS_CONTROL_DATA)
            .register(),
        has_final_offset: protocol
            .field(
                c"Has final offset?",
                c"dcquic.tag.has_final_offset",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(masks::HAS_FINAL_OFFSET)
            .register(),
        key_phase: protocol
            .field(c"Key Phase", c"dcquic.tag.key_phase", BOOLEAN, SEP_DOT, c"")
            .with_mask(masks::KEY_PHASE)
            .register(),
        wire_version: protocol
            .field(
                c"Wire Version",
                c"dcquic.wire_version",
                UINT32,
                BASE_DEC,
                c"dcQUIC wire version",
            )
            .register(),
        path_secret_id: protocol
            .field(
                c"Path Secret ID",
                c"dcquic.path_secret_id",
                BYTES,
                BASE_NONE,
                c"dcQUIC path secret id",
            )
            .register(),
        key_id: protocol
            .field(
                c"Key ID",
                c"dcquic.key_id",
                UINT64,
                BASE_DEC,
                c"dcQUIC key ID",
            )
            .register(),
        source_control_port: protocol
            .field(
                c"Source Control Port",
                c"dcquic.source_control_port",
                UINT16,
                BASE_DEC,
                c"source control port",
            )
            .register(),
        source_stream_port: protocol
            .field(
                c"Source Stream Port",
                c"dcquic.source_stream_port",
                UINT16,
                BASE_DEC,
                c"",
            )
            .register(),
        packet_number: protocol
            .field(
                c"Packet Number",
                c"dcquic.packet_number",
                UINT64,
                BASE_DEC,
                c"packet number",
            )
            .register(),
        payload_len: protocol
            .field(
                c"Payload length",
                c"dcquic.payload_len",
                UINT64,
                BASE_DEC,
                c"payload length",
            )
            .register(),
        next_expected_control_packet: protocol
            .field(
                c"Next expected control packet",
                c"dcquic.next_expected_control_packet",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        control_data_len: protocol
            .field(
                c"Control Data length",
                c"dcquic.control_data_len",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        application_header_len: protocol
            .field(
                c"Application Header length",
                c"dcquic.application_header_len",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        application_header: protocol
            .field(
                c"Application Header",
                c"dcquic.application_header",
                BYTES,
                BASE_NONE,
                c"",
            )
            .register(),
        control_data: protocol
            .field(
                c"Control Data",
                c"dcquic.control_data",
                BYTES,
                BASE_NONE,
                c"",
            )
            .register(),
        payload: protocol
            .field(c"Payload", c"dcquic.payload", BYTES, BASE_NONE, c"")
            .register(),
        auth_tag: protocol
            .field(
                c"Authentication tag",
                c"dcquic.auth_tag",
                BYTES,
                BASE_NONE,
                c"",
            )
            .register(),
        is_bidirectional: protocol
            .field(
                c"Is bidirectional?",
                c"dcquic.is_bidirectional",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(0x1)
            .register(),
        is_reliable: protocol
            .field(
                c"Is reliable?",
                c"dcquic.is_reliable",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(0x2)
            .register(),
        stream_id: protocol
            .field(c"Stream ID", c"dcquic.stream_id", UINT64, BASE_DEC, c"")
            .register(),
        relative_packet_number: protocol
            .field(
                c"Relative Packet Number",
                c"dcquic.relative_packet_number",
                UINT32,
                BASE_DEC,
                c"",
            )
            .register(),
        stream_offset: protocol
            .field(
                c"Stream Payload Offset",
                c"dcquic.stream_payload_offset",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        final_offset: protocol
            .field(
                c"Stream Final Payload Length",
                c"dcquic.stream_final_offset",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        is_stream: protocol
            .field(
                c"Is stream control packet?",
                c"dcquic.is_stream_control",
                BOOLEAN,
                SEP_DOT,
                c"",
            )
            .with_mask(masks::IS_STREAM)
            .register(),
        ack_delay: protocol
            .field(
                c"Ack delay",
                c"dcquic.control.ack_delay",
                RELATIVE_TIME,
                BASE_NONE,
                c"",
            )
            .register(),
        ackd_packet: protocol
            .field(c"Ack Range", c"dcquic.control.ack", UINT64, BASE_DEC, c"")
            .register(),
        ack_range_min: protocol
            .field(c"Min", c"dcquic.control.ack.min", UINT64, BASE_DEC, c"")
            .register(),
        ack_range_max: protocol
            .field(c"Max", c"dcquic.control.ack.min", UINT64, BASE_DEC, c"")
            .register(),
        ect_0_count: protocol
            .field(
                c"ECT(0) count",
                c"dcquic.control.ect0",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        ect_1_count: protocol
            .field(
                c"ECT(1) count",
                c"dcquic.control.ect1",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        ce_count: protocol
            .field(c"CE count", c"dcquic.control.ce", UINT64, BASE_DEC, c"")
            .register(),
        max_data: protocol
            .field(
                c"Max Data",
                c"dcquic.control.max_data",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        close_error_code: protocol
            .field(
                c"Close Error Code",
                c"dcquic.control.close_error_code",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        close_frame_type: protocol
            .field(
                c"Close Frame Type",
                c"dcquic.control.close_frame_type",
                UINT64,
                BASE_DEC,
                c"",
            )
            .register(),
        close_reason: protocol
            .field(
                c"Close Reason",
                c"dcquic.control.close_reason",
                STRING,
                BASE_NONE,
                c"",
            )
            .register(),
        min_key_id: protocol
            .field(
                c"Min KeyId",
                c"dcquic.secret.min_key_id",
                UINT64,
                BASE_DEC,
                c"Minimum not-yet-seen key ID (at the time of this packets sending)",
            )
            .register(),
        rejected_key_id: protocol
            .field(
                c"Rejected KeyId",
                c"dcquic.secret.rejected_key_id",
                UINT64,
                BASE_DEC,
                c"KeyId rejected due to definitively observing replay",
            )
            .register(),
    }
}

mod masks {
    use s2n_quic_dc::packet::{control, datagram, stream};

    pub const IS_STREAM: u64 = control::Tag::IS_STREAM_MASK as _;
    pub const ACK_ELICITING: u64 = datagram::Tag::ACK_ELICITING_MASK as _;
    pub const IS_CONNECTED: u64 = datagram::Tag::IS_CONNECTED_MASK as _;
    pub const HAS_SOURCE_STREAM_PORT: u64 = stream::Tag::HAS_SOURCE_STREAM_PORT as _;
    pub const IS_RECOVERY_PACKET: u64 = stream::Tag::IS_RECOVERY_PACKET as _;
    pub const HAS_CONTROL_DATA: u64 = stream::Tag::HAS_CONTROL_DATA_MASK as _;
    pub const HAS_FINAL_OFFSET: u64 = stream::Tag::HAS_FINAL_OFFSET_MASK as _;

    macro_rules! common_tag {
        ($name:ident) => {{
            // Statically assert that the masks line up between all three packets.
            const _: [(); stream::Tag::$name as usize] = [(); datagram::Tag::$name as usize];
            const _: [(); stream::Tag::$name as usize] = [(); control::Tag::$name as usize];

            datagram::Tag::$name as _
        }};
    }

    pub const HAS_APPLICATION_HEADER: u64 = common_tag!(HAS_APPLICATION_HEADER_MASK);
    pub const KEY_PHASE: u64 = common_tag!(KEY_PHASE_MASK);
}
