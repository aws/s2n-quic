// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//= https://www.rfc-editor.org/rfc/rfc9000#19.20
//# The server uses a HANDSHAKE_DONE frame (type=0x1e) to signal
//# confirmation of the handshake to the client.

macro_rules! handshake_done_tag {
    () => {
        0x1eu8
    };
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HandshakeDone;

impl HandshakeDone {
    pub const fn tag(self) -> u8 {
        handshake_done_tag!()
    }
}

simple_frame_codec!(HandshakeDone {}, handshake_done_tag!());
