// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("transport:frame_sent")]
/// Frame was sent
struct FrameSent {
    packet_header: PacketHeader,
    path_id: u64,
    frame: Frame,
}
