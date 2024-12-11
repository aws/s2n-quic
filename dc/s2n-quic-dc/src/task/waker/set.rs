// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::worker;
use std::{
    sync::{Arc, Mutex},
    task::{Wake, Waker},
};

mod bitset;
use bitset::BitSet;

#[derive(Default)]
pub struct Set {
    state: Arc<State>,
    ready: BitSet,
}

impl Set {
    /// Updates the root waker
    pub fn update_root(&self, waker: &Waker) {
        self.state.root.update(waker);
    }

    /// Registers a waker with the given ID
    pub fn waker(&mut self, id: usize) -> Waker {
        // reserve space in the locally ready set
        self.ready.resize_for_id(id);
        let state = self.state.clone();
        state.ready.lock().unwrap().resize_for_id(id);
        Waker::from(Arc::new(Slot { id, state }))
    }

    /// Returns all of the IDs that are woken
    pub fn drain(&mut self) -> impl Iterator<Item = usize> + '_ {
        core::mem::swap(&mut self.ready, &mut self.state.ready.lock().unwrap());
        self.ready.drain()
    }
}

#[derive(Default)]
struct State {
    root: worker::Waker,
    ready: Mutex<BitSet>,
}

struct Slot {
    id: usize,
    state: Arc<State>,
}

impl Wake for Slot {
    #[inline]
    fn wake(self: Arc<Self>) {
        let mut ready = self.state.ready.lock().unwrap();
        unsafe {
            // SAFETY: the bitset was grown at the time of the call to [`Set::waker`]
            ready.insert_unchecked(self.id)
        }
        drop(ready);
        self.state.root.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn waker_set_test() {
        bolero::check!().with_type::<Vec<u8>>().for_each(|ops| {
            let mut root = Set::default();
            let mut wakers = vec![];

            if let Some(max) = ops.iter().cloned().max() {
                let len = max as usize + 1;
                for i in 0..len {
                    wakers.push(root.waker(i));
                }
            }

            for idx in ops {
                wakers[*idx as usize].wake_by_ref();
            }

            let actual = root.drain().collect::<BTreeSet<_>>();
            let expected = ops.iter().map(|v| *v as usize).collect::<BTreeSet<_>>();
            assert_eq!(actual, expected);
        })
    }
}
