// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::id::{IdMap, LocalSenderId},
    socket::rate::Rate,
    time::precision::Timestamp,
};

pub struct Local {
    edts: IdMap<LocalSenderId, u64>,
    rate: Rate,
}

impl Local {
    pub fn new(socket_count: usize, rate: Rate) -> Self {
        Self {
            edts: IdMap::new(socket_count, 0u64),
            rate,
        }
    }

    pub fn len(&self) -> usize {
        self.edts.len()
    }

    pub fn advance(&mut self, sender_idx: LocalSenderId, now: Timestamp, byte_cost: u64) {
        if sender_idx.as_usize() >= self.edts.len() {
            return;
        }

        let cost_nanos = self.rate.nanos_for_bytes(byte_cost);
        let base = self.edts[sender_idx].max(now.nanos);
        self.edts[sender_idx] = base.saturating_add(cost_nanos);
    }

    #[inline]
    pub fn load_score(&self, sender_idx: LocalSenderId) -> u64 {
        if sender_idx.as_usize() >= self.edts.len() {
            return 0;
        }
        self.edts[sender_idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::id::Id;

    fn rate_10gbps() -> Rate {
        Rate::new(10.0)
    }

    fn ts(nanos: u64) -> Timestamp {
        Timestamp { nanos }
    }

    #[test]
    fn advance_from_zero() {
        let mut edt = Local::new(2, rate_10gbps());
        let idx = LocalSenderId::from_index(0);
        let now = ts(1_000_000_000);

        edt.advance(idx, now, 1000);

        let score = edt.load_score(idx);
        assert!(score > now.nanos);
        assert!(score < now.nanos + 1000);
    }

    #[test]
    fn advance_monotonic() {
        let mut edt = Local::new(1, rate_10gbps());
        let idx = LocalSenderId::from_index(0);
        let now = ts(1_000_000_000);

        edt.advance(idx, now, 5000);
        let first = edt.load_score(idx);

        edt.advance(idx, now, 5000);
        let second = edt.load_score(idx);
        assert!(second > first);
    }

    #[test]
    fn idle_gap_snaps_forward() {
        let mut edt = Local::new(1, rate_10gbps());
        let idx = LocalSenderId::from_index(0);

        edt.advance(idx, ts(1_000_000_000), 10000);
        let old_score = edt.load_score(idx);

        let future_now = ts(5_000_000_000);
        assert!(future_now.nanos > old_score);

        edt.advance(idx, future_now, 1000);
        let new_score = edt.load_score(idx);
        assert!(new_score > future_now.nanos);
        assert!(new_score < future_now.nanos + 1000);
    }

    #[test]
    fn out_of_bounds_is_noop() {
        let mut edt = Local::new(2, rate_10gbps());
        let oob = LocalSenderId::from_index(5);

        edt.advance(oob, ts(1_000_000_000), 1000);
        assert_eq!(edt.load_score(oob), 0);
    }
}
