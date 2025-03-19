// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::queue::Half;
use s2n_quic_core::{probe, varint::VarInt};

probe::define!(
    extern "probe" {
        /// Called when a packet is sent on a queue
        #[link_name = s2n_quic_dc__stream__recv__dispatch__send]
        pub fn on_send(queue_id: VarInt, half: Half, has_overflow: bool);

        /// Called when a number of packets are received on a queue
        #[link_name = s2n_quic_dc__stream__recv__dispatch__recv]
        pub fn on_recv(queue_id: VarInt, half: Half, count: usize);

        /// Called when the receiver has been opened for both halves
        #[link_name = s2n_quic_dc__stream__recv__dispatch__receiver_open]
        pub fn on_receiver_open(queue_id: VarInt);

        /// The half of the receiver has been dropped
        #[link_name = s2n_quic_dc__stream__recv__dispatch__receiver_drop]
        pub fn on_receiver_drop(queue_id: VarInt, half: Half);

        /// Both sides of the receiver has been dropped and the `owner`
        /// is now freeing the descriptor back to the pool
        #[link_name = s2n_quic_dc__stream__recv__dispatch__receiver_free]
        pub fn on_receiver_free(queue_id: VarInt, owner: Half);

        /// Called when a sender is dropped for a queue
        #[link_name = s2n_quic_dc__stream__recv__dispatch__sender_drop]
        pub fn on_sender_drop(queue_id: VarInt);

        /// Called when a queue is closed by the sender
        ///
        /// The queue will not be reopened after this point. Receivers may
        /// still drain the remaining packets in the queue.
        #[link_name = s2n_quic_dc__stream__recv__dispatch__sender_close]
        pub fn on_sender_close(queue_id: VarInt);

        /// Called when the pool is grown
        #[link_name = s2n_quic_dc__stream__recv__dispatch__grow]
        pub fn on_grow(prev_size: usize, next_size: usize);

        /// Called when the pool is draining
        #[link_name = s2n_quic_dc__stream__recv__dispatch__draining]
        pub fn on_draining(total_size: usize, remaining: usize);

        /// Called when the pool is drained
        #[link_name = s2n_quic_dc__stream__recv__dispatch__drained]
        pub fn on_drained(total_size: usize);
    }
);
