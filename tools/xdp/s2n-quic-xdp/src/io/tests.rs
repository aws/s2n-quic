// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    rx::{self, Rx},
    tx::{self, Tx},
};
use crate::{if_xdp::RingFlags, ring, umem::Umem};
use core::{mem::size_of, time::Duration};
use rand::Rng;
use s2n_quic_core::{
    inet::ExplicitCongestionNotification,
    io::{
        rx::{Queue as _, Rx as _},
        tx::{Error, Message, PayloadBuffer, Queue as _, Tx as _},
    },
    sync::atomic_waker,
    xdp::path,
};

/// Tests the s2n-quic-core IO trait implementations by sending packets over spsc channels
#[tokio::test]
async fn tx_rx_test() {
    let frame_count = 16;
    let mut umem = Umem::builder();
    umem.frame_count = frame_count;
    umem.frame_size = 128;
    let umem = umem.build().unwrap();

    let iterations = 10;

    // send a various amount of packets for each test
    for packets in [1, 100, 1000, 10_000] {
        for input_counts in [1, 2] {
            for _ in 0..iterations {
                eprintln!("======================");
                eprintln!("packets: {packets}, input_counts: {input_counts}");

                let mut rx_inputs = vec![];
                let mut tx_outputs = vec![];

                let mut frames = umem.frames();

                for _ in 0..input_counts {
                    let (completion, mut fill) = ring::testing::completion_fill(32);
                    let (mut rx, tx) = ring::testing::rx_tx(16);
                    let (tx_waker, rx_waker) = atomic_waker::pair();

                    // we always need to wakeup
                    rx.set_flags(RingFlags::NEED_WAKEUP);

                    {
                        fill.acquire(u32::MAX);
                        let count = frame_count / input_counts;
                        let frames = (&mut frames).take(count as usize);
                        let (head, tail) = fill.data();
                        for (frame, fill) in frames.zip(head.iter_mut().chain(tail)) {
                            *fill = frame;
                        }
                        fill.release(count);
                    }

                    tx_outputs.push(tx::Channel {
                        tx,
                        driver: tx_waker,
                        completion,
                    });
                    rx_inputs.push(rx::Channel {
                        rx,
                        driver: rx_waker,
                        fill,
                    });
                }

                let send_task = tokio::spawn(send(packets, tx_outputs, umem.clone()));
                recv(packets, rx_inputs, umem.clone()).await;
                send_task.await.unwrap();
            }
        }
    }
}

/// Packets sent over the IO implementations
#[derive(Debug)]
struct Packet {
    pub path: path::Tuple,
    pub ecn: ExplicitCongestionNotification,
    pub counter: u32,
}

/// Make it easy to write the packet to the TX queue
impl Message for Packet {
    type Handle = path::Tuple;

    fn path_handle(&self) -> &Self::Handle {
        &self.path
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.ecn
    }

    fn delay(&mut self) -> Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        self.counter
    }

    fn can_gso(&self, _: usize, _: usize) -> bool {
        false
    }

    fn write_payload(&mut self, mut payload: PayloadBuffer, _gso: usize) -> Result<usize, Error> {
        payload.write(&self.counter.to_be_bytes())
    }
}

/// Sends `count` packets over the TX queue
async fn send(count: u32, outputs: Vec<tx::Channel<atomic_waker::Handle>>, umem: Umem) {
    let state = Default::default();
    let mut tx = Tx::new(outputs, umem, state);

    let mut counter = 0;
    let mut needs_poll = false;
    while counter < count {
        if core::mem::take(&mut needs_poll) && tx.ready().await.is_err() {
            break;
        }

        tx.queue(|queue| {
            let max = queue.capacity().min((count - counter) as usize);
            let count = rand::rng().random_range(0..=max);
            trace!("max: {max}, count: {count}");

            for _ in 0..count {
                let path = path::Tuple::UNSPECIFIED;
                let ecn = ExplicitCongestionNotification::default();
                let packet = Packet { counter, ecn, path };
                counter += 1;
                queue.push(packet).unwrap();
            }

            needs_poll |= !queue.has_capacity();
        });

        // randomly yield to other tasks
        maybe_yield().await;
    }

    trace!("shutting down send");
}

/// Receives raw packets and converts them into [`Packet`]s, putting them on the `output` channel.
async fn recv(packets: u32, inputs: Vec<rx::Channel<atomic_waker::Handle>>, umem: Umem) {
    let mut rx = Rx::new(inputs, umem);
    let mut actual = s2n_quic_core::interval_set::IntervalSet::default();

    while rx.ready().await.is_ok() {
        rx.queue(|queue| {
            queue.for_each(|_datagram, payload| {
                assert_eq!(payload.len(), size_of::<u32>());
                let counter = u32::from_be_bytes(payload.try_into().unwrap());
                trace!("received packet {counter}");
                actual.insert_value(counter).unwrap();
            });
        });

        // randomly yield to other tasks
        maybe_yield().await;
    }

    assert_eq!(
        packets as usize,
        actual.count(),
        "total output packets does not match input"
    );

    trace!("shutting down recv");
}

/// Randomly yields to other tasks
async fn maybe_yield() {
    if rand::rng().random() {
        trace!("yielding");
        tokio::task::yield_now().await;
    }
}
