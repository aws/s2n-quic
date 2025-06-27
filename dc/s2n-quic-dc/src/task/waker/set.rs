// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::worker;
use std::{
    sync::{Arc, Mutex},
    task::{self, Wake, Waker},
};

mod bitset;
use bitset::BitSet;

#[derive(Default)]
pub struct Set {
    state: Arc<State>,
    ready: BitSet,
    local_root: Option<Waker>,
}

impl Set {
    /// Called at the beginning of the `poll` function for the owner of [`Set`]
    #[inline]
    pub fn poll_start(&mut self, cx: &task::Context) {
        let new_waker = cx.waker();

        let root_task_requires_update = if let Some(waker) = self.local_root.as_ref() {
            !waker.will_wake(new_waker)
        } else {
            true
        };

        if root_task_requires_update {
            self.state.root.update(new_waker);
            self.local_root = Some(new_waker.clone());
        }
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
    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = usize> + '_ {
        let mut state = self.state.ready.lock().unwrap();
        core::mem::swap(&mut self.ready, &mut state);
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
        // use `wake_forced` instead of `wake` since we don't use the sleeping status from `worker::Waker``
        self.state.root.wake_forced();
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
