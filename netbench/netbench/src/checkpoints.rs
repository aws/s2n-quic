// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll};
use std::collections::HashSet;

pub trait Checkpoints {
    fn park(&mut self, id: u64) -> Poll<()>;
    fn unpark(&mut self, id: u64, cx: &mut Context);
}

impl Checkpoints for HashSet<u64> {
    fn park(&mut self, id: u64) -> Poll<()> {
        if self.remove(&id) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn unpark(&mut self, id: u64, cx: &mut Context) {
        self.insert(id);
        // notify the task to make more progress
        cx.waker().wake_by_ref();
    }
}
