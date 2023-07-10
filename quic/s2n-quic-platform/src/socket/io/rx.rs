// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{message::Message, socket::ring::Consumer};
use core::task::{Context, Poll};
use s2n_quic_core::{
    event,
    inet::datagram,
    io::rx,
    path::{LocalAddress, MaxMtu},
};

mod probes {
    s2n_quic_core::extern_probe!(
        extern "probe" {
            /// Emitted when a channel tries to acquire messages
            #[link_name = s2n_quic_platform__socket__io__rx__acquire]
            pub fn acquire(channel: *const (), count: u32);

            /// Emitted when a message is read from a channel
            #[link_name = s2n_quic_platform__socket__io__rx__read]
            pub fn read(
                channel: *const (),
                message: u32,
                segment_index: usize,
                segment_size: usize,
            );

            /// Emitted when a message is finished being read from a channel
            #[link_name = s2n_quic_platform__socket__io__rx__finish_read]
            pub fn finish_read(
                channel: *const (),
                message: u32,
                segment_count: usize,
                segment_size: usize,
            );

            /// Emitted when all acquired messages are read from the channel
            #[link_name = s2n_quic_platform__socket__io__rx__release]
            pub fn release(channel: *const (), count: u32);
        }
    );
}

/// Structure for receiving messages from consumer channels
pub struct Rx<T: Message> {
    channels: Vec<Consumer<T>>,
    max_mtu: MaxMtu,
    local_address: LocalAddress,
}

impl<T: Message> Rx<T> {
    #[inline]
    pub fn new(channels: Vec<Consumer<T>>, max_mtu: MaxMtu, local_address: LocalAddress) -> Self {
        Self {
            channels,
            max_mtu,
            local_address,
        }
    }
}

impl<T: Message> rx::Rx for Rx<T> {
    type PathHandle = T::Handle;
    type Queue = RxQueue<'static, T>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let mut is_any_ready = false;
        let mut is_all_closed = true;

        // try to acquire any messages we can from the set of channels
        for channel in self.channels.iter_mut() {
            match channel.poll_acquire(u32::MAX, cx) {
                Poll::Ready(count) => {
                    probes::acquire(channel.as_ptr(), count);
                    is_all_closed = false;
                    is_any_ready = true;
                }
                Poll::Pending => {
                    probes::acquire(channel.as_ptr(), 0);
                    is_all_closed &= !channel.is_open();
                }
            }
        }

        // if all of the channels are closed then shut down the task
        if is_all_closed {
            return Err(()).into();
        }

        // if any have items to be consumed the wake the endpoint up
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

        let mut queue = RxQueue {
            channels: &mut this.channels,
            max_mtu: this.max_mtu,
            local_address: &this.local_address,
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

pub struct RxQueue<'a, T: Message> {
    channels: &'a mut [Consumer<T>],
    max_mtu: MaxMtu,
    local_address: &'a LocalAddress,
}

impl<'a, T: Message> rx::Queue for RxQueue<'a, T> {
    type Handle = T::Handle;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        for channel in self.channels.iter_mut() {
            // one last effort to acquire items if some were received since we last polled
            let len = channel.acquire(u32::MAX);
            let channel_ptr = channel.as_ptr();

            probes::acquire(channel_ptr, len);

            let absolute_index = channel.absolute_index();

            let data = channel.data();
            debug_assert_eq!(data.len(), len as usize);

            for (message_idx, message) in data.iter_mut().enumerate() {
                let message_idx = absolute_index.from_relative(message_idx as _);

                // call the `on_packet` function for each message received
                //
                // NOTE: it's important that we process all of the messages in the queue as the
                //       channel is completely drained here.
                if let Some(message) = message.rx_read(self.local_address) {
                    let mut segment_index = 0;
                    let mut segment_size = 0;
                    message.for_each(|header, payload| {
                        probes::read(channel_ptr, message_idx, segment_index, payload.len());

                        segment_size = segment_size.max(payload.len());

                        on_packet(header, payload);

                        segment_index += 1;
                    });

                    probes::finish_read(channel_ptr, message_idx, segment_index, segment_size as _);
                }

                unsafe {
                    // Safety: the message was allocated with the configured MaxMtu
                    message.reset(self.max_mtu.into());
                }
            }

            // release the messages back to the producer
            probes::release(channel_ptr, len);
            channel.release(len);
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        false
    }
}
