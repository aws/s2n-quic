// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Non-Send intrusive queue channel for busy-poll runtimes.
//!
//! The sender has no backpressure - it can always push entries to the queue.
//! The receiver drains the queue until empty, returning Pending when empty.

use crate::intrusive_queue;
use std::{
    cell::{Cell, UnsafeCell},
    rc::Rc,
    task::Poll,
};

struct Shared<A: intrusive_queue::Adapter> {
    queue: UnsafeCell<intrusive_queue::List<A>>,
    is_open: Cell<bool>,
}

impl<A: intrusive_queue::Adapter> Shared<A> {
    #[inline(always)]
    fn is_alive(&self) -> bool {
        self.is_open.get()
    }
}

pub fn new<T>() -> (
    Sender<intrusive_queue::EntryAdapter<T>>,
    Receiver<intrusive_queue::EntryAdapter<T>>,
) {
    new_with_adapter::<intrusive_queue::EntryAdapter<T>>()
}

pub fn new_with_adapter<A: intrusive_queue::Adapter>() -> (Sender<A>, Receiver<A>) {
    let shared = Rc::new(Shared {
        queue: UnsafeCell::new(intrusive_queue::List::new()),
        is_open: Cell::new(true),
    });
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}

pub struct Sender<A: intrusive_queue::Adapter> {
    shared: Rc<Shared<A>>,
}

impl<A: intrusive_queue::Adapter> Clone for Sender<A> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<A: intrusive_queue::Adapter> super::super::UnboundedSender<A::Pointer> for Sender<A> {
    #[inline(always)]
    fn send(&mut self, value: A::Pointer) -> Result<(), A::Pointer> {
        if !self.shared.is_alive() {
            return Err(value);
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();
            queue.push_back(value);
        }

        Ok(())
    }
}

impl<A: intrusive_queue::Adapter> super::super::Sender<A::Pointer> for Sender<A> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<A::Pointer>,
    ) -> Poll<Result<(), ()>> {
        if !self.shared.is_alive() {
            return Poll::Ready(Err(()));
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();
            let entry = value.assume_init_read();
            queue.push_back(entry);
        }

        Poll::Ready(Ok(()))
    }
}

impl<A: intrusive_queue::Adapter> Sender<A> {
    /// Convert this sender into a list-based sender
    pub fn into_list_sender(self) -> ListSender<A> {
        ListSender { sender: self }
    }
}

/// List-based sender that sends batches of items
pub struct ListSender<A: intrusive_queue::Adapter> {
    sender: Sender<A>,
}

impl<A: intrusive_queue::Adapter> super::super::UnboundedSender<intrusive_queue::List<A>>
    for ListSender<A>
{
    #[inline(always)]
    fn send(&mut self, mut list: intrusive_queue::List<A>) -> Result<(), intrusive_queue::List<A>> {
        if list.is_empty() {
            return Ok(());
        }

        if !self.sender.shared.is_alive() {
            return Err(list);
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.sender.shared.queue.get();
            queue.append(&mut list);
        }

        Ok(())
    }
}

impl<A: intrusive_queue::Adapter> super::super::Sender<intrusive_queue::List<A>> for ListSender<A> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive_queue::List<A>>,
    ) -> Poll<Result<(), ()>> {
        let list = unsafe { value.assume_init_read() };

        if list.is_empty() {
            return Poll::Ready(Ok(()));
        }

        match <Self as super::super::UnboundedSender<intrusive_queue::List<A>>>::send(self, list) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(returned_list) => {
                // Put the list back and signal closed
                value.write(returned_list);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<A: intrusive_queue::Adapter> {
    shared: Rc<Shared<A>>,
}

impl<A: intrusive_queue::Adapter> Drop for Receiver<A> {
    fn drop(&mut self) {
        // Mark the channel as closed so senders can observe it
        self.shared.is_open.set(false);
    }
}

impl<A: intrusive_queue::Adapter> Receiver<A> {
    /// Convert this receiver into a list-based receiver
    pub fn into_list_receiver(self) -> ListReceiver<A> {
        ListReceiver { receiver: self }
    }
}

impl<A: intrusive_queue::Adapter> super::super::Receiver<A::Pointer> for Receiver<A> {
    #[inline(always)]
    fn poll_recv(&mut self, _cx: &mut core::task::Context<'_>) -> Poll<Option<A::Pointer>> {
        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();

            if let Some(entry) = queue.pop_front() {
                return Poll::Ready(Some(entry));
            }
        }

        if !self.shared.is_alive() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}

/// List-based receiver that receives batches of items
pub struct ListReceiver<A: intrusive_queue::Adapter> {
    receiver: Receiver<A>,
}

impl<A: intrusive_queue::Adapter> super::super::Receiver<intrusive_queue::List<A>>
    for ListReceiver<A>
{
    #[inline(always)]
    fn poll_recv(
        &mut self,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<intrusive_queue::List<A>>> {
        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.receiver.shared.queue.get();

            if !queue.is_empty() {
                // Drain all available entries into a list
                let list = core::mem::take(queue);
                return Poll::Ready(Some(list));
            }
        }

        if !self.receiver.shared.is_alive() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}
