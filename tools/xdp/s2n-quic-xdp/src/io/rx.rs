// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{if_xdp::RxTxDescriptor, ring, umem::Umem};
use core::task::{Context, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    event,
    inet::datagram,
    io::rx,
    slice::zip,
    sync::atomic_waker,
    xdp::{decoder, path},
};

#[cfg(feature = "tokio")]
mod tokio_impl;

/// An interface to handle any errors that happen on the RX IO provider
pub trait ErrorLogger: Send {
    /// Called any time the packet could not be decoded.
    ///
    /// Assuming the correct BPF program is loaded, this should never happen. In case it does, the
    /// application should emit an alarm to diagnose the reason why it's happening.
    fn log_invalid_packet(&mut self, bytes: &[u8]);
}

/// Drives the Rx and Fill rings forward
pub trait Driver: 'static {
    fn poll(&mut self, rx: &mut ring::Rx, fill: &mut ring::Fill, cx: &mut Context) -> Option<u32>;
}

impl Driver for atomic_waker::Handle {
    #[inline]
    fn poll(&mut self, rx: &mut ring::Rx, fill: &mut ring::Fill, cx: &mut Context) -> Option<u32> {
        let mut count = 0;

        // iterate twice to avoid race conditions on the waker registration
        for i in 0..2 {
            count = rx.acquire(u32::MAX);
            count = fill.acquire(count).min(count);

            // we have items to receive and fill so return
            if count > 0 {
                break;
            }

            // check to see if the channel is open, if not return
            if !self.is_open() {
                return None;
            }

            // only register the waker the first iteration
            if i > 0 {
                continue;
            }

            trace!("registering waker");
            self.register(cx.waker());
            trace!("waking peer waker");
            self.wake();
        }

        Some(count)
    }
}

pub struct Channel<D: Driver> {
    pub rx: ring::Rx,
    pub fill: ring::Fill,
    pub driver: D,
}

impl<D: Driver> Channel<D> {
    /// Tries to acquire entries in the channel
    ///
    /// Returns None if the channel is closed
    #[inline]
    fn acquire(&mut self, cx: &mut Context) -> Option<u32> {
        self.driver.poll(&mut self.rx, &mut self.fill, cx)
    }

    /// Iterates over all of the acquired entries in the ring and calls `on_packet`
    #[inline]
    fn for_each<F: FnMut(RxTxDescriptor)>(&mut self, mut on_packet: F) {
        // one last effort to acquire any packets
        let len = self.rx.acquire(1);
        let len = self.fill.acquire(len);
        if len == 0 {
            return;
        }

        let rx = self.rx.data();
        let rx = [rx.0, rx.1];

        let fill = self.fill.data();
        let mut fill = [fill.0, fill.1];

        let count = zip(&rx, &mut fill, |rx, fill| {
            on_packet(*rx);
            // send the descriptor to the fill queue
            *fill = (*rx).into();
        });

        trace!("releasing {count} descriptors");

        self.rx.release(count as _);
        self.fill.release(count as _);
    }
}

pub struct Rx<D: Driver> {
    channels: Vec<Channel<D>>,
    umem: Umem,
    error_logger: Option<Box<dyn ErrorLogger>>,
}

impl<D: Driver> Rx<D> {
    /// Creates a RX IO interface for an s2n-quic endpoint
    pub fn new(channels: Vec<Channel<D>>, umem: Umem) -> Self {
        Self {
            channels,
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

impl<D: Driver> rx::Rx for Rx<D> {
    type PathHandle = path::Tuple;
    type Queue = Queue<'static, D>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // poll both channels to make sure we can make progress in both

        let mut is_any_ready = false;
        // assume all of the channels are closed until we get the first open one
        let mut is_all_closed = true;

        for (idx, channel) in self.channels.iter_mut().enumerate() {
            if let Some(count) = channel.acquire(cx) {
                trace!("acquired {count} items from channel {idx}");
                is_all_closed = false;
                is_any_ready |= count > 0;
            } else {
                trace!("channel {idx} closed");
            }
        }

        // if all of the channels are closed then shut down
        if is_all_closed {
            return Err(()).into();
        }

        // wake the endpoint if any of the channels are ready to be processed
        if is_any_ready {
            trace!("rx ready");
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

        let channels = &mut this.channels;
        let umem = &mut this.umem;
        let error_logger = &mut this.error_logger;

        let mut queue = Queue {
            channels,
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

pub struct Queue<'a, D: Driver> {
    channels: &'a mut Vec<Channel<D>>,
    umem: &'a mut Umem,
    error_logger: &'a mut Option<Box<dyn ErrorLogger>>,
}

impl<'a, D: Driver> rx::Queue for Queue<'a, D> {
    type Handle = path::Tuple;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        for (idx, channel) in self.channels.iter_mut().enumerate() {
            trace!("draining channel {idx}");

            channel.for_each(|descriptor| {
                trace!("received descriptor {descriptor:?}");

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
            });
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        false
    }
}
