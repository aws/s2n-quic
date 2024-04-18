// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

pub trait Allocator {
    type Segment: Segment;
    type Retransmission: Segment;

    fn alloc(&mut self) -> Option<Self::Segment>;

    fn get<'a>(&'a self, segment: &'a Self::Segment) -> &'a Vec<u8>;
    fn get_mut<'a>(&'a mut self, segment: &'a Self::Segment) -> &'a mut Vec<u8>;

    fn push(&mut self, segment: Self::Segment);
    fn push_with_retransmission(&mut self, segment: Self::Segment) -> Self::Retransmission;
    fn retransmit(&mut self, segment: Self::Retransmission) -> Self::Segment;
    fn retransmit_copy(&mut self, retransmission: &Self::Retransmission) -> Option<Self::Segment>;

    fn can_push(&self) -> bool;
    fn is_empty(&self) -> bool;
    fn segment_len(&self) -> Option<u16>;

    fn free(&mut self, segment: Self::Segment);
    fn free_retransmission(&mut self, segment: Self::Retransmission);

    fn ecn(&self) -> ExplicitCongestionNotification;
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification);

    fn remote_address(&self) -> SocketAddress;
    fn set_remote_address(&mut self, addr: SocketAddress);
    fn set_remote_port(&mut self, port: u16);

    fn force_clear(&mut self);
}

pub trait Segment: 'static + Send + core::fmt::Debug {
    fn leak(&mut self);
}
