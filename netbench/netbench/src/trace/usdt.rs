// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// if the probe is disabled, then the variables won't be used.
#![allow(unused_variables)]

use super::Trace;
use crate::{timer::Timestamp, units::*};
use probe::probe;

#[derive(Clone, Debug, Default)]
pub struct Usdt {
    connection_id: u64,
}

impl Trace for Usdt {
    #[inline(always)]
    fn enter_connection(&mut self, id: u64) {
        self.connection_id = id;
    }

    #[inline(never)]
    fn send(&mut self, _now: Timestamp, stream_id: u64, len: u64) {
        probe!(netbench, netbench__send, self.connection_id, stream_id, len);
    }

    #[inline(never)]
    fn send_finish(&mut self, _now: Timestamp, stream_id: u64) {
        probe!(
            netbench,
            netbench__send__finish,
            self.connection_id,
            stream_id
        );
    }

    #[inline(never)]
    fn receive(&mut self, _now: Timestamp, stream_id: u64, len: u64) {
        probe!(
            netbench,
            netbench__receive,
            self.connection_id,
            stream_id,
            len
        );
    }

    #[inline(never)]
    fn receive_finish(&mut self, _now: Timestamp, stream_id: u64) {
        probe!(
            netbench,
            netbench__receive__finish,
            self.connection_id,
            stream_id
        );
    }

    #[inline(never)]
    fn accept(&mut self, _now: Timestamp, stream_id: u64) {
        probe!(netbench, netbench__accept, self.connection_id, stream_id);
    }

    #[inline(never)]
    fn open(&mut self, _now: Timestamp, stream_id: u64) {
        probe!(netbench, netbench__open, self.connection_id, stream_id);
    }

    #[inline(never)]
    fn trace(&mut self, _now: Timestamp, id: u64) {
        probe!(netbench, netbench__trace, self.connection_id, id);
    }

    #[inline(never)]
    fn profile(&mut self, now: Timestamp, id: u64, time: Duration) {
        let time = time.as_micros() as u64;
        probe!(netbench, netbench__profile, self.connection_id, id, time);
    }

    #[inline(never)]
    fn park(&mut self, _now: Timestamp, id: u64) {
        probe!(netbench, netbench__park, self.connection_id, id);
    }

    #[inline(never)]
    fn unpark(&mut self, _now: Timestamp, id: u64) {
        probe!(netbench, netbench__unpark, self.connection_id, id);
    }

    #[inline(never)]
    fn connect(&mut self, _now: Timestamp, id: u64, time: Duration) {
        let time = time.as_micros() as u64;
        probe!(netbench, netbench__connect, self.connection_id, id, time);
    }
}
