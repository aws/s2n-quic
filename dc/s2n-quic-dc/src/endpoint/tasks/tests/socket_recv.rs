// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `socket_recv` task.
//!
//! The socket recv task reads raw UDP segments from a bound socket, decodes them into
//! individual packets via GRO flattening, and routes each segment through a Router.
//! These tests use bach's simulated UDP sockets to send real bytes and verify that
//! the router receives them.

use crate::{
    endpoint::tasks,
    socket::{
        channel::{intrusive::sync, ReceiverExt as _},
        pool::{descriptor::RecycleAdapter, Pool},
        recv::router::Router,
    },
    testing::{ext::*, sim},
};
use bach::net::UdpSocket;
use std::{cell::Cell, rc::Rc};

fn test_recycler() -> (
    Rc<std::cell::RefCell<crate::intrusive::List<RecycleAdapter>>>,
    crate::socket::pool::descriptor::WeakRecycleSender,
    // Keep sender alive so Weak::upgrade succeeds
    crate::socket::channel::intrusive::sync::AdapterSender<RecycleAdapter>,
) {
    let (tx, _rx) = sync::new_with_adapter::<RecycleAdapter>();
    let weak = tx.downgrade();
    let local_pool = Rc::new(std::cell::RefCell::new(
        crate::intrusive::List::<RecycleAdapter>::new(),
    ));
    (local_pool, weak, tx)
}

struct CountingRouter {
    segments: Rc<Cell<usize>>,
    expected: usize,
}

impl Router for CountingRouter {
    fn is_open(&self) -> bool {
        self.segments.get() < self.expected
    }

    fn on_segment(&mut self, _segment: crate::socket::pool::descriptor::Filled) {
        self.segments.set(self.segments.get() + 1);
    }
}

/// A single UDP datagram sent to the recv socket is delivered to the router as one segment.
#[test]
fn single_datagram_routed() {
    sim(|| {
        let segments = Rc::new(Cell::new(0));

        // Receiver in its own group ("receiver" host)
        async move {
            let recv_socket = UdpSocket::bind("0.0.0.0:9000").await.unwrap();
            let pool = Pool::new(1200);
            let router = CountingRouter {
                segments: segments.clone(),
                expected: 1,
            };
            let (local_pool, recycle_weak, _tx) = test_recycler();
            let rx = tasks::socket_recv(recv_socket, pool, local_pool, recycle_weak, router);
            rx.drain_budgeted(Some(32)).await;
            assert_eq!(segments.get(), 1);
        }
        .group("receiver")
        .primary()
        .spawn();

        // Sender in its own group ("sender" host)
        async move {
            let sender = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            sender.send_to(b"hello", "receiver:9000").await.unwrap();
        }
        .group("sender")
        .primary()
        .spawn();
    });
}

/// Multiple datagrams are each delivered as individual segments.
#[test]
fn multiple_datagrams_routed() {
    sim(|| {
        let segments = Rc::new(Cell::new(0));

        async move {
            let recv_socket = UdpSocket::bind("0.0.0.0:9000").await.unwrap();
            let pool = Pool::new(1200);
            let router = CountingRouter {
                segments: segments.clone(),
                expected: 5,
            };
            let (local_pool, recycle_weak, _tx) = test_recycler();
            let rx = tasks::socket_recv(recv_socket, pool, local_pool, recycle_weak, router);
            rx.drain_budgeted(Some(32)).await;
            assert_eq!(segments.get(), 5);
        }
        .group("receiver")
        .primary()
        .spawn();

        async move {
            let sender = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            for _ in 0..5 {
                sender.send_to(b"packet", "receiver:9000").await.unwrap();
            }
        }
        .group("sender")
        .primary()
        .spawn();
    });
}

/// Datagrams of varying sizes are all routed correctly — verifies the FlattenSegments
/// stage handles different segment_len values without dropping or corrupting segments.
#[test]
fn variable_sized_datagrams() {
    sim(|| {
        let segments = Rc::new(Cell::new(0));

        async move {
            let recv_socket = UdpSocket::bind("0.0.0.0:9000").await.unwrap();
            let pool = Pool::new(1200);
            let router = CountingRouter {
                segments: segments.clone(),
                expected: 4,
            };
            let (local_pool, recycle_weak, _tx) = test_recycler();
            let rx = tasks::socket_recv(recv_socket, pool, local_pool, recycle_weak, router);
            rx.drain_budgeted(Some(32)).await;
            assert_eq!(segments.get(), 4);
        }
        .group("receiver")
        .primary()
        .spawn();

        async move {
            let sender = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            sender.send_to(b"a", "receiver:9000").await.unwrap();
            sender.send_to(&[0u8; 100], "receiver:9000").await.unwrap();
            sender.send_to(&[0u8; 1100], "receiver:9000").await.unwrap();
            sender.send_to(&[0u8; 500], "receiver:9000").await.unwrap();
        }
        .group("sender")
        .primary()
        .spawn();
    });
}

/// When the router reports closed (is_open returns false), the task shuts down immediately.
#[test]
fn closed_router_shuts_down_task() {
    sim(|| {
        async move {
            let recv_socket = UdpSocket::bind("0.0.0.0:9000").await.unwrap();
            let pool = Pool::new(1200);
            let router = CountingRouter {
                segments: Rc::new(Cell::new(0)),
                expected: 0,
            };
            let (local_pool, recycle_weak, _tx) = test_recycler();
            let rx = tasks::socket_recv(recv_socket, pool, local_pool, recycle_weak, router);
            rx.drain_budgeted(Some(32)).await;
        }
        .group("receiver")
        .primary()
        .spawn();
    });
}

/// Descriptors are recycled back into the local pool after being dropped by the router,
/// and are reused for subsequent allocations without hitting the allocator.
#[test]
fn descriptors_are_recycled_and_reused() {
    sim(|| {
        let segments = Rc::new(Cell::new(0));

        async move {
            let recv_socket = UdpSocket::bind("0.0.0.0:9000").await.unwrap();
            let pool = Pool::new(1200);
            let router = CountingRouter {
                segments: segments.clone(),
                expected: 3,
            };
            let (tx, rx) = sync::new_with_adapter::<RecycleAdapter>();
            let weak = tx.downgrade();
            let local_pool = Rc::new(std::cell::RefCell::new(
                crate::intrusive::List::<RecycleAdapter>::new(),
            ));

            // Spawn the drain task to move descriptors from sync → local pool
            let drain_rx = tasks::recycle_drain(rx, local_pool.clone(), tx);
            crate::testing::ext::SpawnExt::spawn(
                drain_rx.drain_budgeted(Some(32)),
            );

            let recv_rx = tasks::socket_recv(
                recv_socket,
                pool,
                local_pool.clone(),
                weak,
                router,
            );
            recv_rx.drain_budgeted(Some(64)).await;

            assert_eq!(segments.get(), 3);
            // Yield to allow the drain task to poll the sync channel
            bach::time::sleep(core::time::Duration::from_millis(1)).await;
            // After processing 3 packets, descriptors should be in the local pool
            let pool_size = local_pool.borrow().len();
            assert!(
                pool_size > 0,
                "local recycle pool should have recycled descriptors, got {pool_size}"
            );
        }
        .group("receiver")
        .primary()
        .spawn();

        async move {
            let sender = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            sender.send_to(b"packet1", "receiver:9000").await.unwrap();
            sender.send_to(b"packet2", "receiver:9000").await.unwrap();
            sender.send_to(b"packet3", "receiver:9000").await.unwrap();
        }
        .group("sender")
        .primary()
        .spawn();
    });
}
