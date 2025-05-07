// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    socket::recv,
    stream::Actor,
    testing::{ext::*, sim},
};
use bolero::{check, TypeGenerator};
use s2n_quic_core::varint::VarInt;
use std::{collections::BTreeMap, panic::AssertUnwindSafe};

#[derive(Clone, Debug, TypeGenerator)]
enum Op {
    Alloc,
    FreeControl { idx: u16 },
    FreeStream { idx: u16 },
    SendControl { idx: u16 },
    SendStream { idx: u16, inject: bool },
    DropAllocator,
    DropDispatcher,
}

struct Model {
    oracle: Oracle,
    alloc: Option<Allocator>,
    dispatch: Option<Dispatch>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new(Default::default(), false)
    }
}

impl Model {
    fn new(packets: Packets, non_zero: bool) -> Self {
        let stream_cap = 32;
        let control_cap = 8;
        let alloc = if non_zero {
            Allocator::new_non_zero(stream_cap, control_cap)
        } else {
            Allocator::new(stream_cap, control_cap)
        };
        let dispatch = alloc.dispatcher();
        let oracle = Oracle::new(packets);

        Self {
            oracle,
            alloc: Some(alloc),
            dispatch: Some(dispatch),
        }
    }

    fn apply(&mut self, op: &Op) {
        match op {
            Op::Alloc => {
                self.alloc();
            }
            Op::FreeControl { idx } => {
                self.free_control((*idx).into());
            }
            Op::FreeStream { idx } => {
                self.free_stream((*idx).into());
            }
            Op::SendControl { idx } => {
                self.send_control((*idx).into());
            }
            Op::SendStream { idx, inject } => {
                self.send_stream((*idx).into(), *inject);
            }
            Op::DropAllocator => {
                self.alloc = None;
            }
            Op::DropDispatcher => {
                self.dispatch = None;
            }
        }
    }

    fn alloc(&mut self) {
        let Some(alloc) = self.alloc.as_mut() else {
            return;
        };
        let (control, stream) = alloc.alloc_or_grow(None);
        self.oracle.on_alloc(control, stream);
    }

    fn free_control(&mut self, idx: VarInt) {
        let _ = self.oracle.control.remove(&idx);
    }

    fn free_stream(&mut self, idx: VarInt) {
        let _ = self.oracle.stream.remove(&idx);
    }

    fn send_control(&mut self, queue_id: VarInt) {
        let Some(dispatch) = self.dispatch.as_mut() else {
            return;
        };

        let (packet_id, packet) = self.oracle.packets.create();
        let res = dispatch.send_control(queue_id, packet);
        self.oracle.on_control_dispatch(queue_id, packet_id, res);
    }

    fn send_stream(&mut self, queue_id: VarInt, inject: bool) {
        if inject {
            return self.oracle.send_stream_inject(queue_id);
        }

        let Some(dispatch) = self.dispatch.as_mut() else {
            return;
        };

        let (packet_id, packet) = self.oracle.packets.create();
        let res = dispatch.send_stream(queue_id, packet);
        self.oracle.on_stream_dispatch(queue_id, packet_id, res);
    }
}

struct Oracle {
    stream: BTreeMap<VarInt, Stream>,
    control: BTreeMap<VarInt, Control>,
    packets: Packets,
}

impl Oracle {
    fn new(packets: Packets) -> Self {
        Self {
            packets,
            stream: Default::default(),
            control: Default::default(),
        }
    }

    fn on_alloc(&mut self, control: Control, stream: Stream) {
        let queue_id = control.queue_id();
        assert_eq!(queue_id, stream.queue_id(), "queue IDs should match");

        assert!(
            control.try_recv().unwrap().is_none(),
            "queue should be empty"
        );
        assert!(
            stream.try_recv().unwrap().is_none(),
            "queue should be empty"
        );

        assert!(
            self.control.insert(queue_id, control).is_none(),
            "queue ID should be unique"
        );
        assert!(
            self.stream.insert(queue_id, stream).is_none(),
            "queue ID should be unique"
        );
    }

    fn on_control_dispatch(
        &mut self,
        idx: VarInt,
        packet_id: u64,
        result: Result<Option<desc::Filled>, Error>,
    ) {
        let Some(channel) = self.control.get(&idx) else {
            assert!(result.is_err());
            return;
        };
        assert!(result.is_ok());
        let actual = channel.try_recv().unwrap().unwrap();
        assert_eq!(
            actual.payload(),
            packet_id.to_be_bytes(),
            "queue should contain expected packet id"
        );
        assert!(
            channel.try_recv().unwrap().is_none(),
            "queue should be empty now"
        );
    }

    fn on_stream_dispatch(
        &mut self,
        idx: VarInt,
        packet_id: u64,
        result: Result<Option<desc::Filled>, Error>,
    ) {
        let Some(channel) = self.stream.get(&idx) else {
            assert!(result.is_err());
            return;
        };
        assert!(result.is_ok());
        let actual = channel.try_recv().unwrap().unwrap();
        assert_eq!(
            actual.payload(),
            packet_id.to_be_bytes(),
            "queue should contain expected packet id"
        );
        assert!(
            channel.try_recv().unwrap().is_none(),
            "queue should be empty now"
        );
    }

    fn send_stream_inject(&mut self, idx: VarInt) {
        let Some(channel) = self
            .stream
            .get(&idx)
            .or_else(|| self.stream.first_key_value().map(|(_k, v)| v))
        else {
            return;
        };
        let (packet_id, packet) = self.packets.create();
        assert!(channel.push(packet).is_none(), "queue should accept packet");
        let actual = channel.try_recv().unwrap().unwrap();
        assert_eq!(
            actual.payload(),
            packet_id.to_be_bytes(),
            "queue should contain expected packet id"
        );
        if matches!(channel.try_recv(), Ok(Some(_))) {
            panic!("queue should be empty or errored");
        }
    }
}

#[derive(Clone)]
struct Packets {
    packets: recv::pool::Pool,
    packet_id: u64,
}

impl Default for Packets {
    fn default() -> Self {
        Self {
            packets: recv::pool::Pool::new(8, 8),
            packet_id: Default::default(),
        }
    }
}

impl Packets {
    fn create(&mut self) -> (u64, recv::descriptor::Filled) {
        let packet_id = self.packet_id;
        self.packet_id += 1;
        let unfilled = self.packets.alloc_or_grow();
        let packet = unfilled
            .recv_with(|_addr, _cmsg, mut payload| {
                let v = packet_id.to_be_bytes();
                payload[..v.len()].copy_from_slice(&v);
                <std::io::Result<_>>::Ok(v.len())
            })
            .unwrap()
            .next()
            .unwrap();
        (packet_id, packet)
    }
}

#[test]
fn model_test() {
    crate::testing::init_tracing();

    // create a Packet allocator once to avoid setup/teardown costs
    let packets = AssertUnwindSafe(Packets::default());

    check!()
        .with_type::<(bool, Vec<Op>)>()
        .with_test_time(core::time::Duration::from_secs(30))
        .for_each(move |(non_zero, ops)| {
            let mut model = Model::new(packets.clone(), *non_zero);
            for op in ops {
                model.apply(op);
            }
        });
}

/// ensure that freeing an allocator notifies all of the open receivers
#[test]
fn alloc_drop_notify() {
    sim(|| {
        let stream_cap = 1;
        let control_cap = 1;
        let mut alloc = Allocator::new(stream_cap, control_cap);

        for _ in 0..2 {
            let (stream, control) = alloc.alloc_or_grow(None);

            async move {
                stream.recv(Actor::Application).await.unwrap_err();
            }
            .primary()
            .spawn();

            async move {
                control.recv(Actor::Application).await.unwrap_err();
            }
            .primary()
            .spawn();
        }

        async move {
            core::time::Duration::from_millis(100).sleep().await;

            drop(alloc);
        }
        .spawn();
    });
}

#[test]
fn associated_credentials() {
    check!().exhaustive().run(|| {
        let mut alloc = Allocator::new(1, 1);
        let alloc = &mut alloc;
        let mut key_id = VarInt::ZERO;
        let key_id = &mut key_id;

        let mut alloc_one = move || {
            let creds = Credentials {
                id: credentials::Id::default(),
                key_id: *key_id,
            };
            *key_id += 1;
            let (stream, control) = alloc.alloc_or_grow(Some(&creds));
            let keys = alloc.pool.keys();

            move || {
                assert_eq!(keys.get(&creds), Some(control.queue_id()));
                assert_eq!(keys.get(&creds), Some(stream.queue_id()));

                // interleave the order in which we drop channels
                if bolero::any() {
                    drop(stream);
                    assert_eq!(
                        keys.get(&creds),
                        Some(control.queue_id()),
                        "credentials should still match"
                    );
                    drop(control);
                } else {
                    drop(control);
                    assert_eq!(
                        keys.get(&creds),
                        Some(stream.queue_id()),
                        "credentials should still match"
                    );
                    drop(stream);
                }

                assert_eq!(keys.get(&creds), None, "credentials should be removed");
            }
        };

        let mut channels = [alloc_one(), alloc_one()];
        // change the order that channels are freed
        channels.shuffle();

        for channel in channels {
            channel();
        }
    });
}

#[test]
fn stress_test() {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::sync_channel as channel,
        },
        thread::{scope, sleep, yield_now},
        time::Duration,
    };

    crate::testing::init_tracing();

    let mut alloc = Allocator::new(1, 1);
    let (stream_send, stream_recv) = channel(10);
    let (control_send, control_recv) = channel(10);

    scope(|s| {
        static IS_OPEN: AtomicBool = AtomicBool::new(true);

        s.spawn(|| {
            sleep(Duration::from_secs(1));
            IS_OPEN.store(false, Ordering::Relaxed);
        });

        let alloc = s.spawn(move || {
            while IS_OPEN.load(Ordering::Relaxed) {
                let (control, stream) = alloc.alloc_or_grow(None);
                stream_send.send(stream).unwrap();
                control_send.send(control).unwrap();
            }
        });

        s.spawn(move || {
            while let Ok(stream) = stream_recv.recv() {
                yield_now();
                drop(stream);
            }
        });

        s.spawn(move || {
            while let Ok(control) = control_recv.recv() {
                yield_now();
                drop(control);
            }
        });

        alloc.join().unwrap();
    });
}
