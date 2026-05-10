// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::socket::pool::descriptor;
use s2n_quic_core::varint::VarInt;

pub enum Stream {
    FlowValidated,
    Data {
        offset: VarInt,
        fin: bool,
        payload: descriptor::Filled,
    },
    Reset {
        error_code: VarInt,
    },
}

pub enum Control {
    Frames { payload: descriptor::Filled },
    Reset { error_code: VarInt },
}

pub enum Sender {
    Ack {
        local_sender_id: VarInt,
        payload: descriptor::Filled,
    },
}

pub mod queue {
    use crate::flow;

    pub type Allocator = flow::queue::Allocator<super::Stream, super::Control, flow::Handle>;
    pub type Dispatcher = flow::queue::Dispatch<super::Stream, super::Control, flow::Handle>;
    pub type Control = flow::queue::Control<super::Stream, super::Control, flow::Handle>;
    pub type Stream = flow::queue::Stream<super::Stream, super::Control, flow::Handle>;
}
