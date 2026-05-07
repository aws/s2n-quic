// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream2 Reader: Reassembly and flow control on top of the reliable datagram pipeline
//!
//! The Reader receives out-of-order datagrams from the pipeline's flow queue stream channel,
//! reassembles them into an ordered byte stream using the Reassembler from s2n-quic-core,
//! and manages remote flow control by sending MAX_DATA frames to the peer.
//!
//! ## Design
//!
//! The Reader maintains a Reassembler buffer that handles out-of-order data. When datagrams
//! arrive through the flow queue stream channel, they're written into the Reassembler at their
//! offset. The Reassembler handles all the complexity of buffering gaps and delivering
//! contiguous data to the application.
//!
//! Flow control is managed by tracking how much data the application has consumed and
//! periodically sending MAX_DATA frames to increase the sender's window. The initial window
//! is advertised during flow establishment.

use crate::{
    byte_vec::ByteVec,
    datagram::batch::Batch,
    flow,
    packet::datagram::{partial::PartialDatagram, QueuePair, RoutingInfo},
    path::secret::map::Entry as PathSecretEntry,
    pipeline::{reset_error::ResetError, StreamMsg},
    socket::channel,
};
use s2n_quic_core::{
    buffer::{
        self,
        reader::{storage::Infallible, Incremental},
        reassembler::Reassembler,
    },
    state::{event, is},
    task::waker,
    varint::VarInt,
};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, trace};

/// Reader for stream2: handles reassembly and flow control
///
/// Boxed to avoid excessive stack usage when passing around in applications
pub struct Reader(Box<Inner>);

struct Inner {
    /// Channel to send batches to the wheel for control messages
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    /// Stream-side channel for receiving data from the pipeline
    stream_rx: flow::queue::Stream<StreamMsg, crate::pipeline::ControlMsg, flow::Handle>,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Stream identifier
    stream_id: VarInt,
    /// Local queue ID for routing
    local_queue_id: VarInt,
    /// Remote queue ID for routing control frames
    remote_queue_id: VarInt,
    /// Reassembly buffer for out-of-order data
    reassembler: Reassembler,
    /// Remote flow control: maximum offset we've advertised to the sender
    remote_max_data: VarInt,
    /// Window size for flow control
    window_size: u64,
    /// Current status of the reader
    status: Status,
    /// Reset error code if the stream was reset by the peer
    reset_error_code: Option<VarInt>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// Flow is open for reads
    #[default]
    Open,
    /// Reset received from peer
    Reset,
    /// All data received and consumed (FIN reached)
    Complete,
}

impl Status {
    is!(is_open, Open);
    is!(is_reset, Reset);
    is!(is_complete, Complete);
    is!(is_terminal, Reset | Complete);

    event! {
        /// Transition to Reset when reset received
        on_reset(Open => Reset);
        /// Transition to Complete when all data consumed
        on_complete(Open => Complete);
    }
}

impl Reader {
    /// Create a new Reader for a client connection
    pub(crate) fn new_client(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        local_queue_id: VarInt,
        remote_queue_id: VarInt,
        stream_rx: flow::queue::Stream<StreamMsg, crate::pipeline::ControlMsg, flow::Handle>,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();
        let remote_max_data = parameters.remote_max_data;

        Self(Box::new(Inner {
            wheel_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            local_queue_id,
            remote_queue_id,
            reassembler: Reassembler::new(),
            remote_max_data,
            window_size,
            status: Status::Open, // Client starts Open after connect
            reset_error_code: None,
        }))
    }

    /// Create a new Reader for a server connection
    pub(crate) fn new_server(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        local_queue_id: VarInt,
        remote_queue_id: VarInt,
        stream_rx: flow::queue::Stream<StreamMsg, crate::pipeline::ControlMsg, flow::Handle>,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();
        let remote_max_data = parameters.remote_max_data;

        Self(Box::new(Inner {
            wheel_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            local_queue_id,
            remote_queue_id,
            reassembler: Reassembler::new(),
            remote_max_data,
            window_size,
            status: Status::Open, // Server starts Open - validation happened in acceptor
            reset_error_code: None,
        }))
    }

    /// Read data into a buffer
    ///
    /// Returns the number of bytes read. Returns 0 if no data is available or EOF reached.
    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        core::future::poll_fn(|cx| self.poll_read_into(cx, buf)).await
    }

    /// Poll-based read
    pub fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        waker::debug_assert_contract(cx, |cx| self.0.poll_read_into(cx, buf))
    }
}

impl Inner {
    fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        // Check if we're in a terminal state
        if self.status.is_reset() {
            if let Some(error_code) = self.reset_error_code {
                let reset_error: ResetError = error_code.into();
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    reset_error,
                )));
            }
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }

        if self.status.is_complete() {
            // EOF
            return Poll::Ready(Ok(0));
        }

        // Process incoming messages to fill the reassembler
        let _ = self.poll_stream_rx(cx)?;

        // Try to read from the reassembler
        let initial_capacity = buf.remaining_capacity();
        if initial_capacity == 0 {
            return Poll::Ready(Ok(0));
        }

        // Copy from reassembler into destination buffer
        if !self.reassembler.is_empty() {
            let bytes_read = {
                let mut tracker = buf.track_write();
                self.reassembler.infallible_copy_into(&mut tracker);
                tracker.written_len()
            };

            if bytes_read > 0 {
                // Check if we should send a MAX_DATA update based on consumed offset
                self.maybe_send_max_data()?;

                // Check if we've reached completion
                if self.reassembler.is_reading_complete() {
                    self.status.on_complete().ok();
                }

                return Poll::Ready(Ok(bytes_read));
            }
        }

        // No data available right now
        if self.reassembler.is_reading_complete() {
            self.status.on_complete().ok();
            Poll::Ready(Ok(0))
        } else {
            Poll::Pending
        }
    }

    /// Poll the stream_rx channel for incoming messages
    ///
    /// NOTE: `poll_swap` both drains the queue and registers the waker in one call,
    /// so there's no need to loop. If the queue is empty, the waker is registered
    /// and we return Pending.
    fn poll_stream_rx(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.stream_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                // Process all messages in the queue
                for msg in queue {
                    match msg.into_inner() {
                        StreamMsg::Data {
                            offset,
                            mut payload,
                            fin,
                        } => {
                            trace!(
                                stream_id = self.stream_id.as_u64(),
                                offset = offset.as_u64(),
                                len = payload.len(),
                                is_fin = fin,
                                "Received data"
                            );

                            // Create an incremental reader from the BytesMut payload
                            let mut incremental = Incremental::new(offset);
                            let mut reader = match incremental.with_storage(&mut payload, fin) {
                                Ok(r) => r,
                                Err(err) => {
                                    debug!(
                                        stream_id = self.stream_id.as_u64(),
                                        ?err,
                                        "Invalid storage/fin combination"
                                    );
                                    let error_code =
                                        crate::pipeline::reset_error::FRAME_DECODE_ERROR;
                                    self.reset_error_code = Some(error_code);
                                    self.status.on_reset().ok();
                                    let _ = self.send_reset_packet(error_code);
                                    let reset_error: ResetError = error_code.into();
                                    return Poll::Ready(Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        reset_error,
                                    )));
                                }
                            };

                            // Write into the reassembler
                            if let Err(err) = self.reassembler.write_reader(&mut reader) {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    ?err,
                                    "Failed to write to reassembler"
                                );
                                // Protocol error - send reset
                                let error_code = crate::pipeline::reset_error::FRAME_DECODE_ERROR;
                                self.reset_error_code = Some(error_code);
                                self.status.on_reset().ok();
                                let _ = self.send_reset_packet(error_code);
                                let reset_error: ResetError = error_code.into();
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    reset_error,
                                )));
                            }
                        }
                        StreamMsg::FlowValidated => {
                            // Flow validation happens in the acceptor before the Reader is
                            // constructed. We should never receive this message.
                            debug!(
                                stream_id = self.stream_id.as_u64(),
                                "Unexpected FlowValidated message"
                            );
                        }
                        StreamMsg::Reset { error_code } => {
                            debug!(
                                stream_id = self.stream_id.as_u64(),
                                error_code = error_code.as_u64(),
                                "Stream reset by peer"
                            );
                            self.reset_error_code = Some(error_code);
                            self.status.on_reset().ok();
                            let reset_error: ResetError = error_code.into();
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::ConnectionReset,
                                reset_error,
                            )));
                        }
                    }
                }

                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream channel closed",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Check if we should send a MAX_DATA update to the peer
    ///
    /// TODO: Consider using completion notifications for MAX_DATA frames to provide
    /// backpressure and limit the number of inflight updates. This would also allow
    /// waking the application when it's time to send a new frame.
    fn maybe_send_max_data(&mut self) -> io::Result<()> {
        // Calculate how much budget has been consumed from the reassembler
        let consumed = self.reassembler.consumed_len();
        let current_max = self.remote_max_data.as_u64();

        // Send MAX_DATA when we've consumed roughly half the window
        let threshold = current_max.saturating_sub(self.window_size / 2);

        if consumed >= threshold {
            // Calculate new MAX_DATA value
            let new_max_data = consumed + self.window_size;
            let new_max_data = VarInt::new(new_max_data)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "max_data overflow"))?;

            self.send_max_data(new_max_data)?;
            self.remote_max_data = new_max_data;
        }

        Ok(())
    }

    /// Send a MAX_DATA frame to the peer
    fn send_max_data(&mut self, maximum_data: VarInt) -> io::Result<()> {
        use s2n_codec::EncoderValue;
        use s2n_quic_core::frame::MaxData;

        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        // Encode MAX_DATA frame
        let frame = MaxData { maximum_data };
        let encoded_bytes = frame.encode_to_vec();
        let control_data = ByteVec::from(encoded_bytes);

        let flow_control = PartialDatagram::new_datagram(
            RoutingInfo::FlowControl {
                source_sender_id: VarInt::MAX,
                queue_pair: QueuePair {
                    source_queue_id: self.local_queue_id,
                    dest_queue_id: self.remote_queue_id,
                },
                stream_id: self.stream_id,
            },
            control_data,
            ByteVec::new(),
            self.path_secret_entry.clone(),
            None, // No completion notification needed for control frames
        );

        builder
            .try_push(flow_control.into())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

        let batch = builder.finish();
        self.wheel_tx
            .send_entry(batch.into())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

        trace!(
            stream_id = self.stream_id.as_u64(),
            maximum_data = maximum_data.as_u64(),
            "Sent MAX_DATA"
        );

        Ok(())
    }

    /// Send a reset packet to the peer
    fn send_reset_packet(&mut self, error_code: VarInt) -> io::Result<()> {
        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        let reset_packet = PartialDatagram::new_datagram(
            RoutingInfo::FlowReset {
                source_sender_id: VarInt::MAX,
                dest_queue_id: self.remote_queue_id,
                stream_id: self.stream_id,
                reset_target: crate::packet::datagram::ResetTarget::Both,
                error_code,
            },
            ByteVec::new(),
            ByteVec::new(),
            self.path_secret_entry.clone(),
            None,
        );

        builder
            .try_push(reset_packet.into())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

        let batch = builder.finish();
        self.wheel_tx
            .send_entry(batch.into())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

        debug!(
            stream_id = self.stream_id.as_u64(),
            error_code = error_code.as_u64(),
            "Sent FlowReset"
        );

        Ok(())
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for Reader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Use BufMut wrapper to avoid initializing the unfilled portion
        let mut buf = buffer::writer::storage::BufMut::new(buf);
        s2n_quic_core::ready!(self.poll_read_into(cx, &mut buf))?;
        Poll::Ready(Ok(()))
    }
}
