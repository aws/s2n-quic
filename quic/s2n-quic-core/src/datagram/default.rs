// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::{
    connection,
    datagram::{ConnectionInfo, Packet, PreConnectionInfo, ReceiveContext},
    transport::parameters::MaxDatagramFrameSize,
};
use alloc::collections::VecDeque;
use bytes::Bytes;
use core::{
    fmt,
    task::{Context, Poll, Waker},
};

/// Handles configuring unreliable datagram support.
///
/// The datagram endpoint allows users to configure their unreliable datagram
/// behavior. It contains two types, the Sender and Receiver, which are necessary
/// for configuring sending and receiving behavior separately. The default Sender
/// and Receiver behavior can be swapped out by implementing the respective [`Sender`](s2n-quic-core::datagram::traits::Sender) and
/// [`Receiver`](s2n-quic-core::datagram::traits::Receiver) traits.
///
#[derive(Debug, Default)]
pub struct Endpoint {
    send_queue_capacity: usize,
    recv_queue_capacity: usize,
}

impl Endpoint {
    /// Creates a builder for the default datagram endpoint
    pub fn builder() -> EndpointBuilder {
        EndpointBuilder::default()
    }
}

/// A builder for the default datagram endpoint
#[derive(Debug, Default)]
pub struct EndpointBuilder {
    send_queue_capacity: usize,
    recv_queue_capacity: usize,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum BuilderError {
    ZeroCapacity,
}

#[cfg(feature = "std")]
impl std::error::Error for BuilderError {}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ZeroCapacity { .. } => {
                write!(f, "Cannot create a queue with zero capacity")
            }
        }
    }
}

/// Builder for the datagram endpoint
impl EndpointBuilder {
    pub fn with_send_capacity(mut self, capacity: usize) -> Result<Self, BuilderError> {
        if capacity == 0 {
            return Err(BuilderError::ZeroCapacity);
        }
        self.send_queue_capacity = capacity;
        Ok(self)
    }

    pub fn with_recv_capacity(mut self, capacity: usize) -> Result<Self, BuilderError> {
        if capacity == 0 {
            return Err(BuilderError::ZeroCapacity);
        }
        self.recv_queue_capacity = capacity;
        Ok(self)
    }

    pub fn build(self) -> Result<Endpoint, core::convert::Infallible> {
        Ok(Endpoint {
            send_queue_capacity: self.send_queue_capacity,
            recv_queue_capacity: self.recv_queue_capacity,
        })
    }
}

impl super::Endpoint for Endpoint {
    type Sender = Sender;
    type Receiver = Receiver;

    fn create_connection(&mut self, info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (
            Sender::builder()
                .with_capacity(self.send_queue_capacity)
                .with_connection_info(info)
                .build()
                .unwrap(),
            Receiver::builder()
                .with_capacity(self.recv_queue_capacity)
                .with_max_datagram_frame_size(MaxDatagramFrameSize::RECOMMENDED)
                .build()
                .unwrap(),
        )
    }

    fn max_datagram_frame_size(&self, _info: &PreConnectionInfo) -> u64 {
        MaxDatagramFrameSize::RECOMMENDED
    }
}

/// Handles receiving unreliable datagrams.
///
/// Stores the queue of datagrams received from the peer.
/// Old datagrams will be popped off the queue in favor of new datagrams if the
/// queue capacity is reached.
pub struct Receiver {
    queue: VecDeque<Bytes>,
    capacity: usize,
    waker: Option<Waker>,
    max_datagram_frame_size: u64,
    error: Option<connection::Error>,
}

impl Receiver {
    /// Creates a builder for the default datagram receiver
    fn builder() -> ReceiverBuilder {
        ReceiverBuilder::default()
    }

    /// Returns a datagram if there are any on the queue
    pub fn recv_datagram(&mut self) -> Option<Bytes> {
        self.queue.pop_front()
    }

    /// Dequeues a datagram received from the peer.
    ///
    /// # Return value
    ///
    /// - `Poll::Pending` if there are no datagrams to be received on the queue. In this case,
    ///   the caller should retry receiving after the [`Waker`](core::task::Waker) on the provided
    ///   [`Context`](core::task::Context) is notified.
    /// - `Poll::Ready(Datagram)` if there exists a datagram to be received.
    /// - `Poll::Ready(DatagramError)` if a connection error occurred and no more datagrams will be received.
    pub fn poll_recv_datagram(&mut self, cx: &mut Context) -> Poll<Result<Bytes, DatagramError>> {
        if let Some(datagram) = self.queue.pop_front() {
            Poll::Ready(Ok(datagram))
        // If there was some connection-level error we don't take the waker
        // and instead error as there will never be any datagrams to receive.
        } else if let Some(err) = self.error {
            Poll::Ready(Err(DatagramError::ConnectionError { error: err }))
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl super::Receiver for Receiver {
    fn on_datagram(&mut self, _: &ReceiveContext, datagram: &[u8]) {
        if datagram.len() as u64 > self.max_datagram_frame_size {
            return;
        }
        // The oldest datagram on the queue is popped off if the queue is full.
        // Configure this behavior by implementing a custom Receiver for datagrams.
        if self.queue.len() == self.capacity {
            self.queue.pop_front();
        }

        self.queue
            .push_back(bytes::Bytes::copy_from_slice(datagram));
        // Since a datagram was appended to the queue, wake the waker to inform
        // the user that it can receive datagrams now.
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    fn on_connection_error(&mut self, error: connection::Error) {
        self.error = Some(error);
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}

// A builder for the default datagram receiver
///
/// Use to configure a datagram recv queue size and how large of
/// a datagram we can receive.
#[derive(Debug)]
struct ReceiverBuilder {
    queue_capacity: usize,
    max_datagram_frame_size: u64,
}

impl Default for ReceiverBuilder {
    fn default() -> Self {
        Self {
            queue_capacity: 200,
            max_datagram_frame_size: MaxDatagramFrameSize::RECOMMENDED,
        }
    }
}

impl ReceiverBuilder {
    /// Sets the capacity of the datagram receiver queue
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }

    pub fn with_max_datagram_frame_size(mut self, size: u64) -> Self {
        self.max_datagram_frame_size = size;
        self
    }

    /// Builds the datagram receiver
    pub fn build(self) -> Result<Receiver, core::convert::Infallible> {
        Ok(Receiver {
            queue: VecDeque::with_capacity(self.queue_capacity),
            capacity: self.queue_capacity,
            waker: None,
            max_datagram_frame_size: self.max_datagram_frame_size,
            error: None,
        })
    }
}

/// A struct to handle sending unreliable datagrams.
///
/// The Sender struct contains the queue of unreliable datagrams to be sent.
/// During transmission time, we alternate between sending datagrams and sending stream
/// data. This is to ensure there is a balance between the amount of reliable
/// and unreliable data getting sent.
///
/// Datagrams are written to the packet in the order they are added to the queue.
/// A datagram that is too large to fit in the packet will be dropped, unless the
/// packet already contains written datagrams. This attempts to prevent
/// the case where all datagrams are dropped because only a small amount of packet
/// space remains.
///
/// Note that there is currently no expiration date for datagrams to live on the queue.
/// Implement the [`Sender`](s2n-quic-core::datagram::traits::Sender) trait if
/// this behavior is necessary for your use-case.
///
#[derive(Debug)]
pub struct Sender {
    queue: VecDeque<Datagram>,
    capacity: usize,
    min_packet_space: usize,
    max_packet_space: usize,
    smoothed_packet_size: f64,
    waker: Option<Waker>,
    max_datagram_payload: u64,
    error: Option<connection::Error>,
}

#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub struct Datagram {
    pub data: Bytes,
}

#[non_exhaustive]
#[derive(Debug, PartialEq)]
pub enum DatagramError {
    #[non_exhaustive]
    QueueAtCapacity,
    #[non_exhaustive]
    ExceedsPeerTransportLimits,
    #[non_exhaustive]
    ConnectionError { error: connection::Error },
}

impl fmt::Display for DatagramError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::QueueAtCapacity { .. } => {
                write!(f, "Queue does not have room for more datagrams.")
            }
            Self::ExceedsPeerTransportLimits { .. } => {
                write!(
                    f,
                    "Datagram size is larger than peer's transport parameters."
                )
            }
            Self::ConnectionError { .. } => {
                write!(f, "Connection-level error occurred.")
            }
        }
    }
}

impl Sender {
    /// Creates a builder for the default datagram sender
    fn builder() -> SenderBuilder {
        SenderBuilder::default()
    }

    /// Enqueues a datagram for sending it towards the peer.
    ///
    /// # Return value
    ///
    /// - `Poll::Pending` if the datagram's send buffer capacity is currently exhausted
    ///   and the datagram was not added to the queue. In this case, the caller should
    ///   retry sending after the [`Waker`](core::task::Waker) on the provided
    ///   [`Context`](core::task::Context) is notified.
    /// - `Poll::Ready(Ok(()))` if the datagram was enqueued for sending.
    /// - `Poll::Ready(Err(DatagramError))` if an error occurred while trying
    ///   to send the datagram.
    pub fn poll_send_datagram(
        &mut self,
        data: &mut bytes::Bytes,
        cx: &mut Context,
    ) -> Poll<Result<(), DatagramError>> {
        if data.len() as u64 > self.max_datagram_payload {
            return Poll::Ready(Err(DatagramError::ExceedsPeerTransportLimits));
        }

        // If there was some connection-level error the user is not allowed to add
        // datagrams to the queue as they will never be sent.
        if let Some(err) = self.error {
            return Poll::Ready(Err(DatagramError::ConnectionError { error: err }));
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
    ///
    /// # Return value
    /// - `Ok(None)` if the datagram was enqueued for sending
    /// - `Ok(Some(Bytes))` if the queue is at capacity this will be the oldest datagram on the queue
    /// - `Err(DatagramError)` if some error occurred
    pub fn send_datagram_forced(
        &mut self,
        data: bytes::Bytes,
    ) -> Result<Option<Bytes>, DatagramError> {
        if data.len() as u64 > self.max_datagram_payload {
            return Err(DatagramError::ExceedsPeerTransportLimits);
        }

        // If there was some connection-level error the user is not allowed to add
        // datagrams to the queue as they will never be sent.
        if let Some(err) = self.error {
            return Err(DatagramError::ConnectionError { error: err });
        }

        // Pop oldest datagram off the queue if it is at capacity
        let mut oldest = None;
        if self.queue.len() == self.capacity {
            oldest = self.queue.pop_front();
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);

        match oldest {
            Some(datagram) => Ok(Some(datagram.data)),
            None => Ok(None),
        }
    }

    /// Adds datagrams on the queue to be sent
    ///
    /// If the queue is full the newest datagram is not added and an error is returned.
    ///
    /// # Return value
    /// - `Ok()` if the datagram was enqueued for sending
    /// - `Err(DatagramError)` if some error occurred
    pub fn send_datagram(&mut self, data: bytes::Bytes) -> Result<(), DatagramError> {
        if data.len() as u64 > self.max_datagram_payload {
            return Err(DatagramError::ExceedsPeerTransportLimits);
        }

        // If there was some connection-level error the user is not allowed to add
        // datagrams to the queue as they will never be sent.
        if let Some(err) = self.error {
            return Err(DatagramError::ConnectionError { error: err });
        }

        if self.queue.len() == self.capacity {
            return Err(DatagramError::QueueAtCapacity);
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
    pub fn smoothed_packet_space(&self) -> usize {
        self.smoothed_packet_size as usize
    }
}

impl super::Sender for Sender {
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

    fn on_connection_error(&mut self, error: connection::Error) {
        self.error = Some(error);
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}

/// A builder for the default datagram sender
///
/// Use to configure a datagram send queue size
#[derive(Debug)]
struct SenderBuilder {
    queue_capacity: usize,
    max_datagram_payload: u64,
}

impl Default for SenderBuilder {
    fn default() -> Self {
        Self {
            queue_capacity: 200,
            max_datagram_payload: 0,
        }
    }
}

impl SenderBuilder {
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
    pub fn build(self) -> Result<Sender, core::convert::Infallible> {
        Ok(Sender {
            queue: VecDeque::with_capacity(self.queue_capacity),
            capacity: self.queue_capacity,
            max_datagram_payload: self.max_datagram_payload,
            max_packet_space: 0,
            min_packet_space: 0,
            smoothed_packet_size: 0.0,
            waker: None,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::WriteError;
    use core::task::{Context, Poll};
    use futures_test::task::{new_count_waker, noop_waker};

    #[test]
    fn send_datagram_forced() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
            waker: noop_waker(),
        };
        // Create a default sender queue that only holds two elements
        let mut default_sender = Sender::builder()
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
        assert_eq!(result, Ok(Some(bytes::Bytes::from_static(&[1, 2, 3]))));

        // Oldest datagram has been bumped off the queue and the newest two datagrams
        // are there
        let second = default_sender.queue.pop_front().unwrap();
        assert_eq!(second.data[..], [4, 5, 6]);
        let third = default_sender.queue.pop_front().unwrap();
        assert_eq!(third.data[..], [7, 8, 9]);
        assert!(default_sender.queue.is_empty());

        // Connection-level error means new datagrams are not added to the queue
        let conn_err = connection::Error::closed(crate::endpoint::Location::Remote);
        default_sender.error = Some(conn_err);
        assert_eq!(
            default_sender.send_datagram_forced(bytes::Bytes::from_static(&[7, 8, 9])),
            Err(DatagramError::ConnectionError { error: conn_err })
        );
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn send_datagram() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
            waker: noop_waker(),
        };
        // Create a default sender queue that only holds two elements
        let mut default_sender = Sender::builder()
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
            Err(DatagramError::QueueAtCapacity)
        );

        // Check that the first two datagrams are still there
        let first = default_sender.queue.pop_front().unwrap();
        assert_eq!(first.data[..], [1, 2, 3]);
        let second = default_sender.queue.pop_front().unwrap();
        assert_eq!(second.data[..], [4, 5, 6]);
        assert!(default_sender.queue.is_empty());

        // Connection-level error means new datagrams are not added to the queue
        let conn_err = connection::Error::closed(crate::endpoint::Location::Remote);
        default_sender.error = Some(conn_err);
        assert_eq!(
            default_sender.send_datagram(bytes::Bytes::from_static(&[7, 8, 9])),
            Err(DatagramError::ConnectionError { error: conn_err })
        );
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn poll_send_datagram() {
        let conn_info = ConnectionInfo::new(100, noop_waker());
        let mut default_sender = Sender::builder()
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
        crate::datagram::Sender::on_transmit(&mut default_sender, &mut packet);

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

        // Connection-level error means new datagrams are not added to the queue
        let conn_err = connection::Error::closed(crate::endpoint::Location::Remote);
        default_sender.error = Some(conn_err);
        assert_eq!(
            default_sender.poll_send_datagram(&mut bytes::Bytes::from_static(&[7, 8, 9]), &mut cx),
            Poll::Ready(Err(DatagramError::ConnectionError { error: conn_err }))
        );
        assert!(default_sender.queue.is_empty());
    }

    #[test]
    fn retain_datagrams() {
        let conn_info = ConnectionInfo {
            max_datagram_payload: 100,
            waker: noop_waker(),
        };
        let mut default_sender = Sender::builder()
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
        // Here we test that record_capacity_stats() is working as expected. We use
        // a precalculated const for the smoothed_packet_space value.
        const SMOOTHED_PACKET_SPACE: usize = 102;

        let mut default_sender = Sender::builder().build().unwrap();
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
        let conn_info = ConnectionInfo::new(100, noop_waker());
        let mut default_sender = Sender::builder()
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
        crate::datagram::Sender::on_transmit(&mut default_sender, &mut packet);

        // Packet still has capacity to write datagrams
        assert!(packet.remaining_capacity > 0);
        // Send queue is not completely depleted
        assert!(!default_sender.queue.is_empty());
    }

    fn fake_receive_context() -> crate::datagram::ReceiveContext<'static> {
        crate::datagram::ReceiveContext {
            path: crate::event::api::Path {
                local_addr: crate::event::api::SocketAddress::IpV4 {
                    ip: &[0; 4],
                    port: 0,
                },
                local_cid: crate::event::api::ConnectionId { bytes: &[] },
                remote_addr: crate::event::api::SocketAddress::IpV4 {
                    ip: &[0; 4],
                    port: 0,
                },
                remote_cid: crate::event::api::ConnectionId { bytes: &[] },
                id: 0,
                is_active: true,
            },
        }
    }

    #[test]
    fn on_datagram() {
        // Create a receiver with limited capacity
        let mut receiver = Receiver::builder()
            .with_capacity(2)
            .with_max_datagram_frame_size(5)
            .build()
            .unwrap();

        let datagram_0 = vec![1, 2, 3];
        let datagram_1 = vec![4, 5, 6];
        let datagram_2 = vec![7, 8, 9];
        let ctx = fake_receive_context();
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_0);
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_1);
        // Datagram queue will be forced to drop a datagram to receive the newest one
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_2);

        // Oldest datagram has been dropped
        assert_eq!(receiver.queue.pop_front().unwrap(), datagram_1);
        assert_eq!(receiver.queue.pop_front().unwrap(), datagram_2);
        assert!(receiver.queue.pop_front().is_none());

        // Datagram sent by peer is larger than max_datagram_frame_size
        let datagram_3 = vec![10, 11, 12, 13, 14, 15];
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_3);
        // Queue is empty as datagram was not accepted
        assert!(receiver.queue.pop_front().is_none());
    }

    #[test]
    fn recv_datagram() {
        let mut receiver = Receiver::builder().build().unwrap();

        // Calling receive with no datagrams on the queue will result in a None
        assert!(receiver.recv_datagram().is_none());

        // Append a datagram to the receive queue
        receiver
            .queue
            .push_back(bytes::Bytes::from_static(&[1, 2, 3]));

        // Now the user can receive a datagram
        assert_eq!(
            receiver.recv_datagram(),
            Some(bytes::Bytes::from_static(&[1, 2, 3]),)
        );
    }

    #[test]
    fn poll_recv_datagram() {
        // Create a receiver
        let mut receiver = Receiver::builder().build().unwrap();

        let (waker, wake_count) = new_count_waker();
        let mut cx = Context::from_waker(&waker);

        // There are no datagrams on the queue to be received
        assert_eq!(receiver.poll_recv_datagram(&mut cx), Poll::Pending);

        // Adding some datagrams to the queue will wake up the stored waker
        let datagram_0 = vec![1, 2, 3];
        let datagram_1 = vec![4, 5, 6];
        let datagram_2 = vec![7, 8, 9];
        let ctx = fake_receive_context();
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_0);
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_1);
        crate::datagram::Receiver::on_datagram(&mut receiver, &ctx, &datagram_2);

        // Waker was called
        assert_eq!(wake_count, 1);

        assert_eq!(
            receiver.poll_recv_datagram(&mut cx),
            Poll::Ready(Ok(bytes::Bytes::from_static(&[1, 2, 3])))
        );

        // Mock a connection error
        let connection_error = connection::Error::closed(crate::endpoint::Location::Remote);
        receiver.error = Some(connection_error);

        // Continue to recv bytes until we run out and return the connection error
        assert_eq!(
            receiver.poll_recv_datagram(&mut cx),
            Poll::Ready(Ok(bytes::Bytes::from_static(&[4, 5, 6])))
        );

        assert_eq!(
            receiver.poll_recv_datagram(&mut cx),
            Poll::Ready(Ok(bytes::Bytes::from_static(&[7, 8, 9])))
        );

        assert_eq!(
            receiver.poll_recv_datagram(&mut cx),
            Poll::Ready(Err(DatagramError::ConnectionError {
                error: connection_error
            }))
        );
    }

    // The MockPacket mocks writing datagrams to a packet, but is not
    // a fully functional mock. It is used to test the logic in the
    // on_transmit function.
    struct MockPacket {
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
