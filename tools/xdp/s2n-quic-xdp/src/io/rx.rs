// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{RxTxDescriptor, UmemDescriptor},
    umem::Umem,
};
use core::{
    cell::UnsafeCell,
    task::{Context, Poll},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    event,
    inet::datagram,
    io::rx,
    sync::spsc,
    xdp::{decoder, path},
};

pub type Occupied = spsc::Receiver<RxTxDescriptor>;
pub type Free = spsc::Sender<UmemDescriptor>;

/// An interface to handle any errors that happen on the RX IO provider
pub trait ErrorLogger: Send {
    /// Called any time the packet could not be decoded.
    ///
    /// Assuming the correct BPF program is loaded, this should never happen. In case it does, the
    /// application should emit an alarm to diagnose the reason why it's happening.
    fn log_invalid_packet(&mut self, bytes: &[u8]);
}

pub struct Rx {
    channels: UnsafeCell<Vec<(Occupied, Free)>>,
    /// Store a vec of slices on the struct so we don't have to allocate every time `queue` is
    /// called. Since this causes the type to be self-referential it does need a bit of unsafe code
    /// to pull this off.
    slices: UnsafeCell<
        Vec<(
            spsc::RecvSlice<'static, RxTxDescriptor>,
            spsc::SendSlice<'static, UmemDescriptor>,
        )>,
    >,
    umem: Umem,
    error_logger: Option<Box<dyn ErrorLogger>>,
}

impl Rx {
    /// Creates a RX IO interface for an s2n-quic endpoint
    pub fn new(channels: Vec<(Occupied, Free)>, umem: Umem) -> Self {
        let slices = UnsafeCell::new(Vec::with_capacity(channels.len()));
        let channels = UnsafeCell::new(channels);
        Self {
            channels,
            slices,
            umem,
            error_logger: None,
        }
    }

    /// Sets the error logger on the RX IO provider
    pub fn with_error_logger(mut self, error_logger: Box<dyn ErrorLogger>) -> Self {
        self.error_logger = Some(error_logger);
        self
    }
}

impl rx::Rx for Rx {
    type PathHandle = path::Tuple;
    type Queue = Queue<'static>;
    type Error = spsc::ClosedError;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // poll both channels to make sure we can make progress in both

        let mut is_any_ready = false;
        let mut is_all_occupied_closed = true;
        let mut is_all_free_closed = true;

        for (occupied, free) in self.channels.get_mut() {
            let mut is_ready = true;

            macro_rules! ready {
                ($slice:ident, $closed:ident) => {
                    match $slice.poll_slice(cx) {
                        Poll::Ready(Ok(_)) => {
                            $closed = false;
                        }
                        Poll::Ready(Err(_)) => {
                            // defer returning an error until all slices return one
                        }
                        Poll::Pending => {
                            $closed = false;
                            is_ready = false
                        }
                    }
                };
            }

            ready!(occupied, is_all_occupied_closed);
            ready!(free, is_all_free_closed);

            is_any_ready |= is_ready;
        }

        if is_all_occupied_closed || is_all_free_closed {
            return Err(spsc::ClosedError).into();
        }

        if is_any_ready {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        let this: &'static mut Self = unsafe {
            // Safety: As noted in the [transmute examples](https://doc.rust-lang.org/std/mem/fn.transmute.html#examples)
            // it can be used to temporarily extend the lifetime of a reference. In this case, we
            // don't want to use GATs until the MSRV is >=1.65.0, which means `Self::Queue` is not
            // allowed to take generic lifetimes.
            //
            // We are left with using a `'static` lifetime here and encapsulating it in a private
            // field. The `Self::Queue` struct is then borrowed for the lifetime of the `F`
            // function. This will prevent the value from escaping beyond the lifetime of `&mut
            // self`.
            //
            // See https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=9a32abe85c666f36fb2ec86496cc41b4
            //
            // Once https://github.com/aws/s2n-quic/issues/1742 is resolved this code can go away
            core::mem::transmute(self)
        };

        let slices = this.slices.get_mut();

        for (occupied, free) in this.channels.get_mut().iter_mut() {
            if occupied.is_empty() || free.capacity() == 0 {
                continue;
            }

            slices.push((occupied.slice(), free.slice()));
        }

        let umem = &mut this.umem;
        let error_logger = &mut this.error_logger;

        let mut queue = Queue {
            slices,
            umem,
            error_logger,
        };
        f(&mut queue);
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, _error: Self::Error, _events: &mut E) {
        // The only reason we would be returning an error is if a channel closed. This could either
        // be because the endpoint is shutting down or one of the tasks panicked. Either way, we
        // don't know what the cause is here so we don't have any events to emit.
    }
}

pub struct Queue<'a> {
    slices: &'a mut Vec<(
        spsc::RecvSlice<'a, RxTxDescriptor>,
        spsc::SendSlice<'a, UmemDescriptor>,
    )>,
    umem: &'a mut Umem,
    error_logger: &'a mut Option<Box<dyn ErrorLogger>>,
}

impl<'a> rx::Queue for Queue<'a> {
    type Handle = path::Tuple;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        for (occupied, free) in self.slices.iter_mut() {
            // only pop as many items as we have capacity to free them
            while free.capacity() > 0 {
                let descriptor = match occupied.pop() {
                    Some(v) => v,
                    None => break,
                };

                let buffer = unsafe {
                    // Safety: this descriptor should be unique, assuming the tasks are functioning
                    // properly
                    self.umem.get_mut(descriptor)
                };

                // create a decoder from the descriptor's buffer
                let decoder = DecoderBufferMut::new(buffer);

                // try to decode the packet and emit the result
                match decoder::decode_packet(decoder) {
                    Ok(Some((header, payload))) => {
                        on_packet(header, payload.into_less_safe_slice());
                    }
                    Ok(None) | Err(_) => {
                        // This shouldn't happen. If it does, the BPF program isn't properly validating
                        // packets before they get to userspace.
                        if let Some(error_logger) = self.error_logger.as_mut() {
                            error_logger.log_invalid_packet(buffer);
                        }
                    }
                }

                // send the descriptor to the free queue
                let result = free.push(descriptor.into());

                debug_assert!(
                    result.is_ok(),
                    "free queue capacity should always exceed occupied"
                );
            }
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }
}

impl<'a> Drop for Queue<'a> {
    #[inline]
    fn drop(&mut self) {
        // make sure we drop all of the slices to flush our changes
        self.slices.clear();
    }
}
