// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::datagram::{ConnectionInfo, Endpoint, Packet, Receiver, Sender};
use alloc::collections::VecDeque;
use bytes::Bytes;
use core::task::{Context, Poll, Waker};

#[derive(Debug, Default)]
pub struct DatagramEndpoint {
    send_queue_capacity: usize,
}

impl DatagramEndpoint {
    /// Creates a builder for the default datagram endpoint
    pub fn builder() -> EndpointBuilder {
        EndpointBuilder::default()
    }
}

/// A builder for the default datagram endpoint
#[derive(Debug, Default)]
pub struct EndpointBuilder {
    send_queue_capacity: usize,
}

impl EndpointBuilder {
    pub fn with_send_capacity(mut self, capacity: usize) -> Self {
        self.send_queue_capacity = capacity;
        self
    }

    pub fn build(self) -> Result<DatagramEndpoint, core::convert::Infallible> {
        Ok(DatagramEndpoint {
            send_queue_capacity: self.send_queue_capacity,
        })
    }
}

impl Endpoint for DatagramEndpoint {
    type Sender = DefaultSender;
    type Receiver = DefaultReceiver;

    fn create_connection(&mut self, info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (
            DefaultSender::builder()
                .with_capacity(self.send_queue_capacity)
                .with_connection_info(info)
                .build()
                .unwrap(),
            DefaultReceiver(()),
        )
    }
}

pub struct DefaultReceiver(());

impl Receiver for DefaultReceiver {
    fn on_datagram(&self, _datagram: &[u8]) {}
}

#[derive(Debug)]
pub struct DefaultSender {
    pub queue: VecDeque<Datagram>,
    capacity: usize,
    min_packet_space: usize,
    max_packet_space: usize,
    smoothed_packet_size: f64,
    waker: Option<Waker>,
    max_datagram_payload: u64,
}

#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub struct Datagram {
    pub data: Bytes,
}

#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum SendDatagramError {
    QueueAtCapacity,
    ExceedsPeerTransportLimits,
}

impl DefaultSender {
    /// Creates a builder for the default datagram sender
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Enqueues a datagram for sending it towards the peer.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// - `Poll::Pending` if the datagram's send buffer capacity is currently exhausted
    ///   and the datagram was not added to the queue. In this case, the caller should
    ///   retry sending after the [`Waker`](core::task::Waker) on the provided
    ///   [`Context`](core::task::Context) is notified.
    /// - `Poll::Ready(Ok(()))` if the datagram was enqueued for sending.
    /// - `Poll::Ready(Err(SendDatagramError))` if an error occurred while trying
    ///   to send the datagram
    pub fn poll_send_datagram(
        &mut self,
        data: &mut bytes::Bytes,
        cx: &mut Context,
    ) -> Poll<Result<(), SendDatagramError>> {
        if data.len() as u64 > self.max_datagram_payload {
            return Poll::Ready(Err(SendDatagramError::ExceedsPeerTransportLimits));
        }

        if self.queue.len() == self.capacity {
            self.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        let datagram = Datagram {
            data: core::mem::replace(data, bytes::Bytes::new()),
        };
        self.queue.push_back(datagram);
        Poll::Ready(Ok(()))
    }

    /// Adds datagrams on the queue to be sent
    ///
    /// If the datagram queue is at capacity the oldest datagram will be popped
    /// off the queue and returned to make space for the newest datagram.
    pub fn send_datagram_forced(
        &mut self,
        data: bytes::Bytes,
    ) -> Result<Option<Datagram>, SendDatagramError> {
        if data.len() as u64 > self.max_datagram_payload {
            return Err(SendDatagramError::ExceedsPeerTransportLimits);
        }

        // Pop oldest datagram off the queue if it is at capacity
        let mut oldest = None;
        if self.queue.len() == self.capacity {
            oldest = self.queue.pop_front();
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);
        Ok(oldest)
    }

    /// Adds datagrams on the queue to be sent
    ///
    /// If the queue is full the newest datagram is not added and an error is returned.
    pub fn send_datagram(&mut self, data: bytes::Bytes) -> Result<(), SendDatagramError> {
        if data.len() as u64 > self.max_datagram_payload {
            return Err(SendDatagramError::ExceedsPeerTransportLimits);
        }

        if self.queue.len() == self.capacity {
            return Err(SendDatagramError::QueueAtCapacity);
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);
        Ok(())
    }

    /// Filter through the datagrams in the send queue and only keep those that
    /// match a predicate
    pub fn retain_datagrams<F>(&mut self, f: F)
    where
        F: FnMut(&Datagram) -> bool,
    {
        self.queue.retain(f);
    }

    fn record_capacity_stats(&mut self, capacity: usize) {
        if capacity < self.min_packet_space || self.min_packet_space == 0 {
            self.min_packet_space = capacity;
        }
        if capacity > self.max_packet_space {
            self.max_packet_space = capacity;
        }

        // https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        self.smoothed_packet_size =
            7.0 / 8.0 * self.smoothed_packet_size + 1.0 / 8.0 * capacity as f64;
    }

    /// Returns the largest packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn max_packet_space(&self) -> usize {
        self.max_packet_space
    }

    /// Returns the smallest packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn min_packet_space(&self) -> usize {
        self.min_packet_space
    }

    /// Returns a smoothed calculation of the size of packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn smoothed_packet_space(&self) -> f64 {
        self.smoothed_packet_size
    }
}

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Cede space to stream data when datagrams are not prioritized
        if packet.has_pending_streams() && !packet.datagrams_prioritized() {
            return;
        }
        self.record_capacity_stats(packet.remaining_capacity());
        let mut has_written = false;
        while packet.remaining_capacity() > 0 {
            if let Some(datagram) = self.queue.pop_front() {
                // Ensure there is enough space in the packet to send a datagram
                if packet.remaining_capacity() >= datagram.data.len() {
                    match packet.write_datagram(&datagram.data) {
                        Ok(()) => has_written = true,
                        Err(_error) => {
                            // TODO emit datagram dropped event
                            continue;
                        }
                    }
                    // Since a datagram was popped off the queue, wake the
                    // stored waker if we have one to let the application know
                    // that there is space on the queue for more datagrams.
                    if let Some(w) = self.waker.take() {
                        w.wake();
                    }
                } else {
                    // This check keeps us from popping all the datagrams off the
                    // queue when packet space remaining is smaller than the datagram.
                    if has_written {
                        self.queue.push_front(datagram);
                        return;
                    } else {
                        // TODO emit datagram dropped event
                    }
                }
            } else {
                // If there are no datagrams on the queue we return
                return;
            }
        }
    }

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        !self.queue.is_empty()
    }
}

/// A builder for the default datagram sender
///
/// Use to configure a datagram send queue size
#[derive(Debug)]
pub struct Builder {
    queue_capacity: usize,
    max_datagram_payload: u64,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            queue_capacity: 200,
            max_datagram_payload: 0,
        }
    }
}

impl Builder {
    /// Sets the capacity of the datagram sender queue
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }

    /// Gives the default sender relevant connection info
    pub fn with_connection_info(mut self, connection_info: &ConnectionInfo) -> Self {
        self.max_datagram_payload = connection_info.max_datagram_payload;
        self
    }

    /// Builds the datagram sender into a provider
    pub fn build(self) -> Result<DefaultSender, core::convert::Infallible> {
        Ok(DefaultSender {
            queue: VecDeque::with_capacity(self.queue_capacity),
            capacity: self.queue_capacity,
            max_datagram_payload: self.max_datagram_payload,
            max_packet_space: 0,
            min_packet_space: 0,
            smoothed_packet_size: 0.0,
            waker: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::{ConnectionInfo, WriteError};
    use core::task::{Context, Poll};
    use futures_test::task::new_count_waker;

    #[test]
    fn send_datagram_forced() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
        };
        // Create a default sender queue that only holds two elements
        let mut default_sender = DefaultSender::builder()
            .with_capacity(2)
            .with_connection_info(&conn_info)
            .build()
            .unwrap();
        let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
        let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
        let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
        assert_eq!(default_sender.send_datagram_forced(datagram_0), Ok(None));
        assert_eq!(default_sender.send_datagram_forced(datagram_1), Ok(None));
        // Queue has reached capacity so oldest datagram is returned
        let result = default_sender.send_datagram_forced(datagram_2);
        assert_eq!(
            result,
            Ok(Some(Datagram {
                data: bytes::Bytes::from_static(&[1, 2, 3])
            }))
        );

        // Oldest datagram has been bumped off the queue and the newest two datagrams
        // are there
        let second = default_sender.queue.pop_front().unwrap();
        assert_eq!(second.data[..], [4, 5, 6]);
        let third = default_sender.queue.pop_front().unwrap();
        assert_eq!(third.data[..], [7, 8, 9]);
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn send_datagram() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
        };
        // Create a default sender queue that only holds two elements
        let mut default_sender = DefaultSender::builder()
            .with_capacity(2)
            .with_connection_info(&conn_info)
            .build()
            .unwrap();
        let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
        let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
        let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
        assert_eq!(default_sender.send_datagram(datagram_0), Ok(()));
        assert_eq!(default_sender.send_datagram(datagram_1), Ok(()));
        // Attempting to send a third datagram will result in an error, since the queue
        // is at capacity
        assert_eq!(
            default_sender.send_datagram(datagram_2),
            Err(SendDatagramError::QueueAtCapacity)
        );

        // Check that the first two datagrams are still there
        let first = default_sender.queue.pop_front().unwrap();
        assert_eq!(first.data[..], [1, 2, 3]);
        let second = default_sender.queue.pop_front().unwrap();
        assert_eq!(second.data[..], [4, 5, 6]);
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn poll_send_datagram() {
        let conn_info = ConnectionInfo::new(100);
        let mut default_sender = DefaultSender::builder()
            .with_capacity(2)
            .with_connection_info(&conn_info)
            .build()
            .unwrap();
        let mut datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
        let mut datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
        let mut datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);

        let (waker, wake_count) = new_count_waker();
        let mut cx = Context::from_waker(&waker);

        assert_eq!(
            default_sender.poll_send_datagram(&mut datagram_0, &mut cx),
            Poll::Ready(Ok(()))
        );

        assert_eq!(
            default_sender.poll_send_datagram(&mut datagram_1, &mut cx),
            Poll::Ready(Ok(()))
        );

        // Waker has not been set up yet
        assert!(default_sender.waker.is_none());

        // Queue is at capacity
        assert_eq!(
            default_sender.poll_send_datagram(&mut datagram_2, &mut cx),
            Poll::Pending
        );

        // Since queue is at capacity default_sender is now storing a waker that will
        // alert when the queue has more space
        assert!(default_sender.waker.is_some());

        let mut packet = MockPacket {
            remaining_capacity: 10,
            has_pending_streams: false,
            datagrams_prioritized: false,
        };
        default_sender.on_transmit(&mut packet);

        // Waker was called
        assert_eq!(wake_count, 1);

        // Now datagrams can be added to the queue as there is space
        let mut datagram_3 = bytes::Bytes::from_static(&[10, 11, 12]);
        assert_eq!(
            default_sender.poll_send_datagram(&mut datagram_3, &mut cx),
            Poll::Ready(Ok(()))
        );

        // Check that all datagrams we expect are on the queue
        let datagram = default_sender.queue.pop_front().unwrap();
        assert_eq!(datagram.data[..], [10, 11, 12]);
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn retain_datagrams() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
        };
        let mut default_sender = DefaultSender::builder()
            .with_capacity(3)
            .with_connection_info(&conn_info)
            .build()
            .unwrap();
        let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
        let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
        let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
        assert_eq!(default_sender.send_datagram_forced(datagram_0), Ok(None));
        assert_eq!(default_sender.send_datagram_forced(datagram_1), Ok(None));
        assert_eq!(default_sender.send_datagram_forced(datagram_2), Ok(None));

        // Keep only the third datagram
        default_sender.retain_datagrams(|datagram| datagram.data[..] == [7, 8, 9]);
        let first = default_sender.queue.pop_front().unwrap();
        assert_eq!(first.data[..], [7, 8, 9]);
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn record_capacity_stats() {
        const SMOOTHED_PACKET_SPACE: f64 = 102.3193359375;

        let mut default_sender = DefaultSender::builder().build().unwrap();
        default_sender.record_capacity_stats(100);
        default_sender.record_capacity_stats(100);
        default_sender.record_capacity_stats(200);
        default_sender.record_capacity_stats(500);

        assert_eq!(default_sender.max_packet_space(), 500);
        assert_eq!(default_sender.min_packet_space(), 100);
        assert_eq!(
            default_sender.smoothed_packet_space(),
            SMOOTHED_PACKET_SPACE
        );
    }

    #[test]
    // Check that our default on_transmit function doesn't continue to pop datagrams
    // off the send queue if the remaining packet space is too small to send datagrams.
    fn has_written_test() {
        let conn_info = ConnectionInfo::new(100);
        let mut default_sender = DefaultSender::builder()
            .with_connection_info(&conn_info)
            .build()
            .unwrap();
        let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
        let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
        assert_eq!(default_sender.send_datagram_forced(datagram_0), Ok(None));
        assert_eq!(default_sender.send_datagram_forced(datagram_1), Ok(None));

        // Packet size is just enough to write the first datagram with some
        // room left over, but not enough to write the second.
        let mut packet = MockPacket {
            remaining_capacity: 5,
            has_pending_streams: false,
            datagrams_prioritized: false,
        };
        default_sender.on_transmit(&mut packet);

        // Packet still has capacity to write datagrams
        assert!(packet.remaining_capacity > 0);
        // Send queue is not completely depleted
        assert!(!default_sender.queue.is_empty());
    }

    // The MockPacket mocks writing datagrams to a packet, but is not
    // a fully functional mock. It is used to test the logic in the
    // on_transmit function.
    pub struct MockPacket {
        has_pending_streams: bool,
        datagrams_prioritized: bool,
        remaining_capacity: usize,
    }

    impl crate::datagram::Packet for MockPacket {
        fn remaining_capacity(&self) -> usize {
            self.remaining_capacity
        }

        fn write_datagram(&mut self, data: &[u8]) -> Result<(), WriteError> {
            if data.len() > self.remaining_capacity {
                return Err(WriteError::ExceedsPacketCapacity);
            }
            self.remaining_capacity -= data.len();
            Ok(())
        }

        fn has_pending_streams(&self) -> bool {
            self.has_pending_streams
        }

        fn datagrams_prioritized(&self) -> bool {
            self.datagrams_prioritized
        }
    }
}
