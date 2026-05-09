// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Worker infrastructure for distributing packets across send/recv sockets.

use crate::{
    counter::Counter,
    intrusive_queue::Entry,
    packet,
    socket::{
        channel::{self, UnboundedSender},
        pool::descriptor,
        recv::router::Router,
    },
};

// ── Worker-Socket Channel ──────────────────────────────────────────────────

/// A specialized channel for distributing frame queues to sockets within a worker.
///
/// Uses a single sync channel per worker that feeds multiple unsync channels per socket,
/// minimizing lock contention. The sender locks once to push to the appropriate socket
/// queue, and the worker-local receiver locks once to swap out all queues for local dispatch.
pub(crate) mod socket_channel {
    use crate::{intrusive_queue::Queue, stream3::frame::Frame};
    use std::sync::{Arc, Mutex};

    struct WorkerQueues {
        queues: Mutex<Vec<Queue<Frame>>>,
    }

    #[derive(Clone)]
    pub struct Sender {
        socket_idx: usize,
        queues: Arc<WorkerQueues>,
    }

    impl Sender {
        pub fn send_queue(&self, queue: Queue<Frame>) {
            let mut queues = self.queues.queues.lock().unwrap();
            queues[self.socket_idx].prepend(&mut { queue });
        }
    }

    #[derive(Clone)]
    pub struct Receiver {
        queues: Arc<WorkerQueues>,
        num_sockets: usize,
    }

    impl Receiver {
        /// Drain all socket queues and dispatch to their respective local channels.
        ///
        /// Performs a single lock to grab all pending frames for all sockets, then
        /// dispatches them to the provided unsync senders inline.
        pub fn drain_to<S>(&self, senders: &mut [S])
        where
            S: crate::socket::channel::UnboundedSender<Queue<Frame>>,
        {
            debug_assert_eq!(senders.len(), self.num_sockets);

            let mut queues = self.queues.queues.lock().unwrap();
            for (queue, sender) in queues.iter_mut().zip(senders.iter_mut()) {
                if !queue.is_empty() {
                    let mut swapped = Queue::new();
                    core::mem::swap(queue, &mut swapped);
                    let _ = sender.send(swapped);
                }
            }
        }
    }

    /// Create a worker-socket channel with the given number of sockets.
    ///
    /// Returns (senders, receiver) where senders[i] sends to socket i.
    pub fn new(num_sockets: usize) -> (Vec<Sender>, Receiver) {
        let queues = Arc::new(WorkerQueues {
            queues: Mutex::new((0..num_sockets).map(|_| Queue::new()).collect()),
        });

        let senders = (0..num_sockets)
            .map(|socket_idx| Sender {
                socket_idx,
                queues: queues.clone(),
            })
            .collect();

        let receiver = Receiver {
            queues,
            num_sockets,
        };

        (senders, receiver)
    }
}

// ── Packet Router ──────────────────────────────────────────────────────────

/// Routes all decoded packets to a single channel for processing.
///
/// There is no separate control packet format — ACKs are frames inside regular
/// datagram packets, so everything goes through one dispatch path.
pub(crate) struct ChannelRouter<D> {
    pub tx: D,
    pub decode_error_counter: Counter,
}

impl<D> Router for ChannelRouter<D>
where
    D: channel::UnboundedSender<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>>,
{
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        packet: packet::datagram::decoder::Packet<descriptor::Filled>,
    ) {
        let _ = self.tx.send(packet.into());
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
    }

    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: s2n_quic_core::inet::SocketAddress,
        segment: descriptor::Filled,
    ) {
        self.decode_error_counter.add(1);
        tracing::debug!(
            ?error,
            %remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
