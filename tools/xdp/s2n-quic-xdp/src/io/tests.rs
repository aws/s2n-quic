// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    rx::{self, Rx},
    tx::{self, Tx},
};
use crate::umem::Umem;
use core::{
    future::Future,
    mem::size_of,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use pin_project_lite::pin_project;
use rand::prelude::*;
use s2n_quic_core::{
    inet::ExplicitCongestionNotification,
    io::{
        rx::{Queue as _, Rx as _},
        tx::{Error, Message, PayloadBuffer, Queue as _, Tx as _},
    },
    sync::spsc,
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

    // send a various amount of packets for each test
    for packets in [1, 100, 1000, 10_000] {
        for input_counts in [1, 2] {
            eprintln!("packets: {packets}, input_counts: {input_counts}");

            let mut rx_inputs = vec![];
            let mut tx_outputs = vec![];

            let mut frames = umem.frames();

            for _ in 0..input_counts {
                let (mut rx_free, tx_free) = spsc::channel(32);
                let (tx_occupied, rx_occupied) = spsc::channel(16);

                let mut rx_frames = (&mut frames).take((frame_count / input_counts) as usize);
                rx_free.slice().extend(&mut rx_frames).unwrap();

                tx_outputs.push((tx_free, tx_occupied));
                rx_inputs.push((rx_occupied, rx_free));
            }

            let (input, tx_input) = spsc::channel(16);
            let (rx_output, output) = spsc::channel(32);

            tokio::spawn(packet_gen(packets, input));
            tokio::spawn(send(tx_outputs, umem.clone(), tx_input));
            tokio::spawn(recv(rx_inputs, umem.clone(), rx_output));
            packet_checker(packets, output).await;
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

/// Generates `count` packets into the output channel
async fn packet_gen(count: u32, mut output: spsc::Sender<Packet>) {
    for counter in 0..count {
        if output.acquire().await.is_err() {
            return;
        }

        let path = path::Tuple::UNSPECIFIED;
        let ecn = ExplicitCongestionNotification::default();

        let packet = Packet { counter, ecn, path };

        trace!("generating packet {packet:?}");

        output.slice().push(packet).unwrap();

        // randomly yield to other tasks
        maybe_yield().await;
    }

    trace!("shutting down packet_gen");
}

/// Sends packets over the TX queue from an input channel
async fn send(
    outputs: Vec<(tx::Free, tx::Occupied)>,
    umem: Umem,
    mut input: spsc::Receiver<Packet>,
) {
    let state = Default::default();
    let mut tx = Tx::new(outputs, umem, state);

    loop {
        let res = select(input.acquire(), tx.ready()).await;

        trace!("send select result: {res:?}");

        // close the sender if we got an error
        match res {
            (Some(Err(_)), _) | (_, Some(Err(_))) => break,
            _ => {}
        }

        tx.queue(|queue| {
            trace!("tx queue has capacity: {}", queue.has_capacity());
            let mut slice = input.slice();
            while queue.has_capacity() {
                if let Some(packet) = slice.pop() {
                    trace!("sending packet {packet:?}");
                    queue.push(packet).unwrap();
                } else {
                    return;
                }
            }
        });

        // randomly yield to other tasks
        maybe_yield().await;
    }

    trace!("send finishing");

    let channels = tx.consume();

    let free: Vec<_> = channels
        .into_iter()
        .map(|(mut free, occupied)| {
            // notify the recv task that there aren't going to be any more packets sent
            drop(occupied);

            async move {
                // drain the free queue so the `recv` task doesn't shut down prematurely
                while free.acquire().await.is_ok() {
                    free.slice().clear();
                }
            }
        })
        .collect();

    // wait until all of the futures finish
    futures::future::join_all(free).await;

    trace!("shutting down send");
}

/// Receives raw packets and converts them into [`Packet`]s, putting them on the `output` channel.
async fn recv(inputs: Vec<(rx::Occupied, rx::Free)>, umem: Umem, mut output: spsc::Sender<Packet>) {
    let mut rx = Rx::new(inputs, umem);

    while rx.ready().await.is_ok() {
        trace!("recv ready");

        rx.queue(|queue| {
            let mut slice = output.slice();
            let _ = slice.sync();
            queue.for_each(|datagram, payload| {
                let path = datagram.path;
                let ecn = datagram.ecn;
                assert_eq!(payload.len(), size_of::<u32>());
                let counter = u32::from_be_bytes(payload.try_into().unwrap());
                let packet = Packet { path, ecn, counter };
                trace!("received packet {packet:?}");

                slice.push(packet).expect("the packet checker task ");
            });
        });

        // randomly yield to other tasks
        maybe_yield().await;
    }

    trace!("shutting down recv");
}

/// Checks that the received [`Packet`]s match the expected values
async fn packet_checker(total: u32, mut output: spsc::Receiver<Packet>) {
    let mut actual = s2n_quic_core::interval_set::IntervalSet::default();

    while output.acquire().await.is_ok() {
        let mut output = output.slice();
        while let Some(packet) = output.pop() {
            trace!("output packet recv: {packet:?}");

            actual.insert_value(packet.counter).unwrap();
        }

        // we want to consume the output queue as fast as possible so the `recv` task doesn't have
        // to block on the checker
    }

    assert_eq!(
        total as usize,
        actual.count(),
        "total output packets does not match input"
    );
}

/// Randomly yields to other tasks
async fn maybe_yield() {
    if thread_rng().gen() {
        tokio::task::yield_now().await;
    }
}

/// Selects either or both tasks and returns their results
async fn select<A: Future, B: Future>(a: A, b: B) -> (Option<A::Output>, Option<B::Output>) {
    Select { a, b }.await
}

pin_project!(
    struct Select<A, B> {
        #[pin]
        a: A,
        #[pin]
        b: B,
    }
);

impl<A: Future, B: Future> Future for Select<A, B> {
    type Output = (Option<A::Output>, Option<B::Output>);

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        let mut is_ready = false;

        macro_rules! ready {
            ($value:expr) => {
                match $value {
                    Poll::Ready(v) => {
                        is_ready = true;
                        Some(v)
                    }
                    Poll::Pending => None,
                }
            };
        }

        let a = ready!(this.a.poll(cx));
        let b = ready!(this.b.poll(cx));

        if is_ready {
            Poll::Ready((a, b))
        } else {
            Poll::Pending
        }
    }
}
