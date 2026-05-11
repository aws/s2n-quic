// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Reader: Reassembly and flow control
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

// TODOs:
//
// Correctness:
//
// * poll_stream_rx processes all messages in the queue but short-circuits on Reset/error.
//   Messages after the Reset in the same queue batch are silently dropped. This is probably
//   fine (reset is terminal), but any data messages preceding the reset in the queue should
//   still be written to the reassembler before we transition to Reset. Currently they are,
//   since we iterate in order and break on Reset, but if the queue ordering isn't guaranteed
//   this could lose data.
//
// * maybe_send_max_data uses consumed_len as the basis for the threshold check, but
//   consumed_len only advances when contiguous data is copied out. If the application reads
//   slowly while data arrives out-of-order, we could buffer large amounts in the reassembler
//   without ever sending MAX_DATA, which is correct (don't open the window for more data we
//   can't consume), but it means the sender will stall even though the reassembler has room.
//   We should consider whether buffered (non-contiguous) data should contribute to the
//   threshold.
//
// * No cap on reassembler memory. The Reassembler can buffer up to remote_max_data worth of
//   out-of-order data. If the window is large (6-7 MiB) and all of it arrives out-of-order,
//   the reassembler holds it all. This is by design but should be monitored — the actual
//   memory bound is the window size.
//
// Flow control:
//
// * Auto-tune window_size based on application drain rate. Currently fixed. If the
//   application drains slowly, shrink the window to reduce buffering. If it drains quickly,
//   grow the window to avoid sender stalls. This is the Reader-side analog of the Writer's
//   local budget auto-tuning.
//
// * Consider using completion notifications for MAX_DATA frames to provide backpressure and
//   limit the number of inflight MAX_DATA updates. This would also allow waking the
//   application when a new MAX_DATA is actually sent (vs queued).
//
// * The threshold for sending MAX_DATA (`consumed >= current_max - window/2`) can fire
//   repeatedly on every read once crossed. Each call to maybe_send_max_data recalculates and
//   may send another update even if consumed hasn't advanced since the last one. The fix is
//   to only send when the new MAX_DATA would actually increase the advertised value.
//
// Performance:
//
// * poll_read_into calls poll_stream_rx and then tries to copy out. If poll_stream_rx
//   returns Pending and the reassembler already has buffered data, we still try to copy
//   (which is fine). But if poll_stream_rx returns an error, we return the error immediately
//   without first draining any already-buffered contiguous data. On Reset, this may discard
//   data that was already reliably delivered and buffered.
//
// Observability:
//
// * No mechanism to expose reassembler buffering stats (gaps, buffered bytes, etc.) to the
//   application or metrics layer. Could be useful for diagnosing throughput issues.
//
// Testing:
//
// * Deterministic tests using bach for: out-of-order reassembly, FIN handling with gaps,
//   MAX_DATA generation and pacing, reset mid-stream, drop semantics (STOP_SENDING), and
//   interaction between slow application reads and flow control.

use crate::{
    byte_vec::ByteVec,
    intrusive_queue::Queue,
    packet::datagram::{QueuePair, ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
    stream3::{
        endpoint::{
            msg,
            reset_error::{self, ResetError},
        },
        frame::{self, Frame, Header, PriorityInput, SubmissionSender, DEFAULT_TTL},
    },
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    buffer::{
        self,
        duplex::Interposer,
        reader::{storage::Infallible as _, Incremental},
        reassembler::Reassembler,
        writer::Storage as _,
        writer::Writer as _,
    },
    frame::MaxData,
    ready,
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

pub struct Reader(Box<Inner>);

struct Inner {
    /// Channel to submit frames to the wheel
    frame_tx: SubmissionSender,
    /// Stream-side channel for receiving data from the pipeline
    stream_rx: msg::queue::Stream,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Stream identifier
    stream_id: VarInt,
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
    /// Server-only: awaiting client validation before releasing credits
    PendingValidation,
    /// Flow is open for reads
    #[default]
    Open,
    /// Reset received from peer
    Reset,
    /// All data received and consumed (FIN reached)
    Complete,
}

impl Status {
    is!(is_pending_validation, PendingValidation);
    is!(is_open, Open);
    is!(is_reset, Reset);
    is!(is_complete, Complete);
    is!(is_terminal, Reset | Complete);

    event! {
        on_validated(PendingValidation => Open);
        on_reset(PendingValidation | Open => Reset);
        on_complete(Open => Complete);
    }
}

impl Reader {
    pub(crate) fn new_client(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let remote_max_data = parameters.local_recv_max_data;
        let window_size = remote_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data,
            window_size,
            status: Status::Open,
            reset_error_code: None,
        }))
    }

    pub(crate) fn new_server(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            status: Status::Open,
            reset_error_code: None,
        }))
    }

    pub(crate) fn new_server_pending(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            status: Status::PendingValidation,
            reset_error_code: None,
        }))
    }

    pub async fn validate(&mut self) -> io::Result<()> {
        core::future::poll_fn(|cx| self.0.poll_validate(cx)).await
    }

    pub(crate) fn send_reset(&mut self, error_code: VarInt) {
        if self.0.status.is_terminal() {
            return;
        }
        let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
        self.0.status.on_reset().ok();
    }

    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        core::future::poll_fn(|cx| self.poll_read_into(cx, buf)).await
    }

    pub fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        waker::debug_assert_contract(cx, |cx| self.0.poll_read_into(cx, buf))
    }
}

impl Inner {
    fn poll_validate(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.status.is_pending_validation() {
            return Poll::Ready(Ok(()));
        }

        let mut app_buf = buffer::writer::storage::Empty;
        match self.poll_stream_rx(cx, &mut app_buf)? {
            Poll::Ready(()) => {
                if self.status.is_pending_validation() {
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        let mut tracker = buf.track_write();
        let _ = self.poll_stream_rx(cx, &mut tracker)?;

        if self.status.is_pending_validation() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stream not yet validated - call validate() first",
            )));
        }

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

        if tracker.has_remaining_capacity() {
            self.reassembler.infallible_copy_into(&mut tracker);
        }

        let bytes_read = tracker.written_len();

        self.maybe_send_max_data()?;

        if self.reassembler.is_reading_complete() {
            debug!(
                stream_id = self.stream_id.as_u64(),
                final_size = ?self.reassembler.final_size(),
                consumed_len = self.reassembler.consumed_len(),
                "Reader complete - all data consumed"
            );
            self.status.on_complete().ok();
            return Poll::Ready(Ok(bytes_read));
        }

        if bytes_read > 0 {
            Poll::Ready(Ok(bytes_read))
        } else {
            Poll::Pending
        }
    }

    fn poll_stream_rx<S>(&mut self, cx: &mut Context, app_buf: &mut S) -> Poll<io::Result<()>>
    where
        S: buffer::writer::Storage + ?Sized,
    {
        let interpose = !self.status.is_pending_validation();

        match self.stream_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                for msg in queue {
                    match msg.into_inner() {
                        msg::Stream::Data {
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

                            let mut incremental = Incremental::new(offset);
                            let mut reader = match incremental.with_storage(&mut payload, fin) {
                                Ok(r) => r,
                                Err(err) => {
                                    debug!(
                                        stream_id = self.stream_id.as_u64(),
                                        ?err,
                                        "Invalid storage/fin combination"
                                    );
                                    return self.protocol_error();
                                }
                            };

                            if let Err(err) =
                                write_data_reader(&mut self.reassembler, &mut reader, app_buf, interpose)
                            {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    ?err,
                                    "Failed to write to reassembler"
                                );
                                return self.protocol_error();
                            }
                        }
                        msg::Stream::FlowValidated => {
                            if self.status.on_validated().is_ok() {
                                debug!(stream_id = self.stream_id.as_u64(), "Flow validated");
                            } else {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    "FlowValidated received in unexpected state"
                                );
                            }
                        }
                        msg::Stream::Reset { error_code } => {
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

    fn protocol_error(&mut self) -> Poll<io::Result<()>> {
        let error_code = reset_error::FRAME_DECODE_ERROR;
        self.reset_error_code = Some(error_code);
        self.status.on_reset().ok();
        let _ = self.send_reset_frame(error_code, ResetTarget::Both);
        let reset_error: ResetError = error_code.into();
        Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, reset_error)))
    }

    fn maybe_send_max_data(&mut self) -> io::Result<()> {
        if let Some(final_size) = self.reassembler.final_size() {
            if self.remote_max_data.as_u64() >= final_size {
                return Ok(());
            }
        }

        let consumed = self.reassembler.consumed_len();
        let current_max = self.remote_max_data.as_u64();
        let threshold = current_max.saturating_sub(self.window_size / 2);

        if consumed >= threshold {
            let new_max_data = consumed + self.window_size;
            let new_max_data = VarInt::new(new_max_data)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "max_data overflow"))?;

            self.send_max_data_frame(new_max_data)?;
            self.remote_max_data = new_max_data;
        } else {
            trace!(
                stream_id = self.stream_id.as_u64(),
                consumed,
                current_max,
                threshold,
                window_size = self.window_size,
                "maybe_send_max_data: below threshold, not sending"
            );
        }

        Ok(())
    }

    fn send_max_data_frame(&mut self, maximum_data: VarInt) -> io::Result<()> {
        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            return Ok(());
        };

        let frame = MaxData { maximum_data };
        let encoded_bytes = frame.encode_to_vec();
        let control_data = ByteVec::from(encoded_bytes);

        let frame = Frame {
            source_sender_id: VarInt::MAX,
            header: Header::FlowControl {
                queue_pair: QueuePair {
                    source_queue_id: self.stream_rx.queue_id(),
                    dest_queue_id: remote_queue_id,
                },
                stream_id: self.stream_id,
            },
            payload: control_data,
            path_secret_entry: self.path_secret_entry.clone(),
            completion: None,
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        self.send_frame(frame)?;

        trace!(
            stream_id = self.stream_id.as_u64(),
            maximum_data = maximum_data.as_u64(),
            "Sent MAX_DATA"
        );

        Ok(())
    }

    fn send_reset_frame(
        &mut self,
        error_code: VarInt,
        reset_target: ResetTarget,
    ) -> io::Result<()> {
        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            return Ok(());
        };

        let frame = Frame {
            source_sender_id: VarInt::MAX,
            header: Header::FlowReset {
                dest_queue_id: remote_queue_id,
                stream_id: self.stream_id,
                reset_target,
                error_code,
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: None,
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        self.send_frame(frame)?;

        debug!(
            stream_id = self.stream_id.as_u64(),
            error_code = error_code.as_u64(),
            ?reset_target,
            "Sent FlowReset"
        );

        Ok(())
    }

    fn send_frame(&mut self, frame: Frame) -> io::Result<()> {
        let mut input = PriorityInput::default();
        input.push(frame.into());
        self.frame_tx
            .send_batch(input)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame channel closed"))
    }
}

#[inline]
fn write_data_reader<S, R>(
    reassembler: &mut Reassembler,
    reader: &mut R,
    app_buf: &mut S,
    interpose: bool,
) -> Result<(), buffer::Error<R::Error>>
where
    S: buffer::writer::Storage + ?Sized,
    R: buffer::reader::Reader + ?Sized,
{
    if interpose && reassembler.is_empty() {
        let mut interposer = Interposer::new(app_buf, reassembler);
        interposer.read_from(reader)
    } else {
        reassembler.write_reader(reader)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        debug!(
            stream_id = self.0.stream_id.as_u64(),
            status = ?self.0.status,
            final_size = ?self.0.reassembler.final_size(),
            consumed_len = self.0.reassembler.consumed_len(),
            total_received_len = self.0.reassembler.total_received_len(),
            is_writing_complete = self.0.reassembler.is_writing_complete(),
            is_reading_complete = self.0.reassembler.is_reading_complete(),
            "Reader dropping"
        );

        if std::thread::panicking() {
            let error_code = reset_error::ABNORMAL_TERMINATION;
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
            debug!(
                stream_id = self.0.stream_id.as_u64(),
                "Reader dropped during panic - sent FlowReset"
            );
        } else if !self.0.reassembler.is_writing_complete() && !self.0.status.is_reset() {
            let error_code = reset_error::STOP_SENDING;
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Stream);
            debug!(
                stream_id = self.0.stream_id.as_u64(),
                "Reader dropped before FIN received - sent STOP_SENDING"
            );
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for Reader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut buf = buffer::writer::storage::BufMut::new(buf);
        ready!(self.poll_read_into(cx, &mut buf))?;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::{msg, write_data_reader, Reader};
    use crate::{
        flow,
        path::secret::map::Entry as PathSecretEntry,
        stream3::frame::{Frame, SubmissionSender},
    };
    use bytes::BytesMut;
    use core::task::Poll;
    use s2n_quic_core::{
        buffer::Reassembler,
        endpoint,
        stream::testing::Data,
        task::waker,
        varint::VarInt,
    };
    use std::{io, net::SocketAddr};

    fn test_frame_tx() -> SubmissionSender {
        let (frame_tx, _frame_rx) = crate::stream3::frame::submission_channel(1);
        frame_tx
    }

    fn filled_payload(data: &[u8]) -> BytesMut {
        BytesMut::from(data)
    }

    fn test_reader(msg: msg::Stream) -> Reader {
        let stream_id = VarInt::from_u8(1);
        let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let path_secret_entry = PathSecretEntry::fake_deterministic(peer, endpoint::Type::Client);
        let handle = flow::Handle::client(stream_id, path_secret_entry.clone());
        let allocator = msg::queue::Allocator::new();
        let (_control, stream_rx) = allocator
            .alloc(handle, Some(VarInt::from_u8(2)))
            .expect("queue alloc should succeed");
        stream_rx.push(msg.into());

        Reader::new_client(test_frame_tx(), path_secret_entry, stream_id, stream_rx)
    }

    #[test]
    fn write_data_reader_bypasses_reassembler_for_in_order_data() {
        let mut reassembler = Reassembler::new();
        let mut reader = Data::new(8);
        let mut app_buf: Vec<u8> = Vec::new();

        write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

        assert_eq!(app_buf, Data::send_one_at(0, 8));
        assert_eq!(reassembler.consumed_len(), 8);
        assert_eq!(reassembler.final_size(), Some(8));
        assert!(reassembler.is_empty());
        assert!(reassembler.is_reading_complete());
    }

    #[test]
    fn write_data_reader_keeps_out_of_order_data_in_reassembler() {
        let mut reassembler = Reassembler::new();
        let mut reader = Data::new(8);
        let mut app_buf: Vec<u8> = Vec::new();

        reader.seek_forward(4);
        write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

        assert!(app_buf.is_empty());
        assert_eq!(reassembler.consumed_len(), 0);
        assert_eq!(reassembler.total_received_len(), 0);
        assert!(reassembler.is_empty());
        assert!(!reassembler.is_reading_complete());

        reassembler.write_at(0u32.into(), &Data::send_one_at(0, 4)).unwrap();
        assert_eq!(reassembler.len(), 8);
    }

    #[test]
    fn write_data_reader_does_not_interpose_when_reassembler_has_head_data() {
        let mut reassembler = Reassembler::new();
        let mut reader = Data::new(8);
        let mut app_buf: Vec<u8> = Vec::new();

        reassembler.write_at(0u32.into(), &Data::send_one_at(0, 4)).unwrap();
        reader.seek_forward(4);

        write_data_reader(&mut reassembler, &mut reader, &mut app_buf, true).unwrap();

        assert!(app_buf.is_empty());
        assert_eq!(reassembler.len(), 8);
        assert_eq!(reassembler.total_received_len(), 8);
        assert!(!reassembler.is_empty());
    }

    #[test]
    fn poll_read_into_counts_direct_interposer_writes() -> io::Result<()> {
        let expected = Data::send_one_at(0, 8);
        let mut reader = test_reader(msg::Stream::Data {
            offset: VarInt::ZERO,
            fin: true,
            payload: filled_payload(&expected),
        });
        let waker = waker::noop();
        let mut cx = core::task::Context::from_waker(&waker);
        let mut out = Vec::new();

        match reader.poll_read_into(&mut cx, &mut out) {
            Poll::Ready(Ok(len)) => assert_eq!(len, 8),
            other => panic!("unexpected first poll result: {other:?}"),
        }
        assert_eq!(out, expected);
        assert!(reader.0.status.is_complete());

        Ok(())
    }
}
