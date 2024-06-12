// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{copy_to_rust, Buffer},
    field::Registration,
    value::Parsed,
    wireshark::Item,
};
use std::{ffi::CStr, sync::OnceLock};

mod buffer;
mod dissect;
mod field;
mod value;
/// This wraps the underlying sys APIs in structures that support a cfg(test) mode that doesn't rely on Wireshark.
mod wireshark;
/// These are bindgen-generated bindings from bindgen 4.2.5.
/// Allow warnings since we don't control the bindgen generation process enough for warnings to be worthwhile to fix.
#[allow(warnings)]
mod wireshark_sys;

#[cfg(test)]
mod test;

#[no_mangle]
#[used]
static plugin_version: [std::ffi::c_char; 4] = [b'0' as _, b'.' as _, b'1' as _, b'\0' as _];

// When bumping, make sure that the bindgen bindings are updated to the new version.
#[no_mangle]
#[used]
static plugin_want_major: std::ffi::c_int = 4;

// When bumping, make sure that the bindgen bindings are updated to the new version.
#[no_mangle]
#[used]
static plugin_want_minor: std::ffi::c_int = 2;

#[no_mangle]
pub extern "C" fn plugin_register() {
    static PLUGIN: wireshark_sys::proto_plugin = wireshark_sys::proto_plugin {
        register_protoinfo: Some(field::proto_register),
        register_handoff: Some(proto_reg_handoff),
    };

    unsafe {
        wireshark_sys::proto_register_plugin(&PLUGIN);
    }
}

static STREAM_DISSECTOR: OnceLock<DissectorHandle> = OnceLock::new();

struct DissectorHandle(wireshark_sys::dissector_handle_t);

// Probably a lie, but just an opaque ID anyway.
// Wireshark owns all the threading for the plugin so this is hopefully OK.
unsafe impl Send for DissectorHandle {}
unsafe impl Sync for DissectorHandle {}

unsafe extern "C" fn proto_reg_handoff() {
    wireshark_sys::heur_dissector_add(
        c"udp".as_ptr(),
        Some(dissect_heur_udp),
        c"dcQUIC over UDP".as_ptr(),
        concat!(env!("PLUGIN_NAME_LOWER"), "\0").as_ptr() as *const _,
        field::get().protocol,
        wireshark_sys::heuristic_enable_e_HEURISTIC_ENABLE,
    );

    wireshark_sys::heur_dissector_add(
        c"tcp".as_ptr(),
        Some(dissect_heur_tcp),
        c"dcQUIC over TCP".as_ptr(),
        concat!(env!("PLUGIN_NAME_LOWER"), "_tcp\0").as_ptr() as *const _,
        field::get().protocol,
        wireshark_sys::heuristic_enable_e_HEURISTIC_ENABLE,
    );

    STREAM_DISSECTOR.get_or_init(|| {
        DissectorHandle(
            wireshark_sys::create_dissector_handle_with_name_and_description(
                Some(dissect_heur_tcp),
                field::get().protocol,
                concat!(env!("PLUGIN_NAME_LOWER"), "_tcp_stream\0").as_ptr() as *const _,
                c"dcQUIC stream".as_ptr(),
            ),
        )
    });
}

unsafe extern "C" fn dissect_heur_udp(
    tvb: *mut wireshark_sys::tvbuff_t,
    mut pinfo: *mut wireshark_sys::_packet_info,
    proto: *mut wireshark_sys::_proto_node,
    _: *mut std::ffi::c_void,
) -> i32 {
    let fields = field::get();

    let packet = copy_to_rust(tvb);
    let mut buffer = Buffer::new(tvb, &packet);

    let mut accepted_offset = 0;
    let mut info = vec![];

    while !buffer.is_empty() {
        let Some(tag) = buffer.consume() else {
            break;
        };
        let (mut tree, mut root) = register_root_node(proto, &buffer, fields);
        let Some(()) =
            dissect::udp_segment(&mut tree, &mut root, fields, tag, &mut buffer, &mut info)
        else {
            break;
        };

        accepted_offset = buffer.offset;
    }

    // Didn't look like a dcQUIC packet.
    if accepted_offset == 0 {
        return 0;
    }

    if !info.is_empty() {
        // add a NULL byte
        info.push(0);
        clear_info(pinfo);
        pinfo.append_text(CStr::from_ptr(info.as_ptr() as *const _));
    }

    set_protocol(pinfo, c"dcQUIC");

    accepted_offset as _
}

unsafe extern "C" fn dissect_heur_tcp(
    tvb: *mut wireshark_sys::tvbuff_t,
    mut pinfo: *mut wireshark_sys::_packet_info,
    proto: *mut wireshark_sys::_proto_node,
    _: *mut std::ffi::c_void,
) -> i32 {
    let fields = field::get();

    let packet = copy_to_rust(tvb);
    let mut buffer = Buffer::new(tvb, &packet);

    let mut accepted_offset = 0;
    let mut info = vec![];

    while !buffer.is_empty() {
        let stream_frame_start = buffer.offset;
        let Some(tag) = buffer.consume() else {
            break;
        };

        let (mut tree, mut root) = register_root_node(proto, &buffer, fields);
        root.append_text(c" Stream");
        let parse_res = dissect::stream(&mut tree, fields, tag, &mut buffer, &mut info);
        wireshark_sys::proto_item_set_len(root, (buffer.offset - stream_frame_start) as i32);
        if parse_res.is_none() {
            // Start parsing again from the head of this stream...
            (*pinfo).desegment_offset = stream_frame_start as _;
            (*pinfo).desegment_len = wireshark_sys::DESEGMENT_ONE_MORE_SEGMENT;
            break;
        }

        accepted_offset = buffer.offset;

        // If we successfully parsed, then mark this conversation as being dissected by us
        // going forward.
        let conversation = wireshark_sys::find_or_create_conversation(pinfo);
        wireshark_sys::conversation_set_dissector(conversation, STREAM_DISSECTOR.get().unwrap().0);
    }

    // Didn't look like a dcQUIC segment.
    if accepted_offset == 0 {
        return 0;
    }

    if !info.is_empty() {
        // add a NULL byte
        info.push(0);
        clear_info(pinfo);
        pinfo.append_text(CStr::from_ptr(info.as_ptr() as *const _));
    }

    set_protocol(pinfo, c"TCP/dcQUIC");

    accepted_offset as _
}

unsafe fn register_root_node(
    proto: *mut wireshark_sys::_proto_node,
    buffer: &Buffer,
    fields: &Registration,
) -> (
    *mut wireshark_sys::_proto_node,
    *mut wireshark_sys::_proto_node,
) {
    let ti = wireshark_sys::proto_tree_add_item(
        proto,
        fields.protocol,
        buffer.tvb,
        0,
        buffer.packet.len() as _,
        wireshark_sys::ENC_BIG_ENDIAN,
    );
    let tree = wireshark_sys::proto_item_add_subtree(ti, fields.all_subtree);
    (tree, ti)
}

unsafe fn set_protocol(pinfo: *mut wireshark_sys::_packet_info, protocol: &'static CStr) {
    // set_str doesn't copy out protocol so it must be static
    wireshark_sys::col_set_str(
        (*pinfo).cinfo,
        wireshark_sys::COL_PROTOCOL as _,
        protocol.as_ptr(),
    );
}

unsafe fn clear_info(pinfo: *mut wireshark_sys::_packet_info) {
    wireshark_sys::col_clear((*pinfo).cinfo, wireshark_sys::COL_INFO as _);
}
