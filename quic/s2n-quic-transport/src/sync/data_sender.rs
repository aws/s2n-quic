use crate::{
    contexts::{OnTransmitError, WriteContext},
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
};
use alloc::collections::VecDeque;
use bytes::{Buf, Bytes};
use s2n_quic_core::{
    ack_set::AckSet, frame::MaxPayloadSizeForFrame, packet::number::PacketNumber, varint::VarInt,
};

/// Manages the outgoing flow control window for a sending data on a particular
/// data stream.
pub trait OutgoingDataFlowController {
    /// Tries to acquire a flow control window for the described chunk of data.
    /// The implementation must return the **maximum** (exclusive) offset up to
    /// which the data sender is allowed to send.
    fn acquire_flow_control_window(&mut self, min_offset: VarInt, size: usize) -> VarInt;

    /// Returns `true` if sending data on the `Stream` was blocked because the
    /// the call to `acquire_flow_control_window` did not return any available
    /// window. This means not even the request for the minimum window size could
    /// be fulfilled.
    fn is_blocked(&self) -> bool;

    /// Clears the `is_blocked` flag which is stored inside the `FlowController`.
    /// The next call to `is_blocked` will return `None`, until another call to
    /// `acquire_flow_control_window` will move it back into the blocked state.
    fn clear_blocked(&mut self);

    /// Signals the flow controller that no further data will be submitted on
    /// the stream and therefore no further flow control window will be requested.
    fn finish(&mut self);
}

/// Writes chunks of data into frames.
pub trait ChunkToFrameWriter: Default {
    /// The type of the Stream ID which needs to get embedded in outgoing frames.
    /// This is generic, since not all frames use the same Stream identifier.
    /// E.g. crypto streams do not utilize a stream identifier. For those,
    /// `StreamId` could be defined to `()`.
    type StreamId: Copy;

    /// Provides an upper bound for the frame size which is required to serialize
    /// a given chunk of data. This is estimation does not not need to be exact.
    /// The actual frame size might be lower, but is never allowed to be higher.
    fn get_max_frame_size(&self, stream_id: Self::StreamId, data_size: usize) -> usize;

    // Check how much data we can fit into that amount of space,
    // given our other known variables. The amount of possible
    // payload is different between whether this is the last frame
    // in a packet or another frame, since we do not have to write
    // length information in the last frame.
    fn max_payload_size(
        &self,
        stream_id: Self::StreamId,
        max_frame_size: usize,
        offset: VarInt,
    ) -> MaxPayloadSizeForFrame;

    /// Creates a QUIC frame out of a chunk of data, and writes it using the
    /// provided [`WriteContext`].
    /// The method returns the `PacketNumber` of the packet containing the value
    /// if the write was successful, and `None` otherwise.
    fn write_value_as_frame<W: WriteContext>(
        &self,
        stream_id: Self::StreamId,
        offset: VarInt,
        data: &[u8],
        is_last_frame: bool,
        is_fin: bool,
        context: &mut W,
    ) -> Option<PacketNumber>;
}

/// Describes a chunk of bytes which has to be transmitted to the peer
#[derive(Debug, Copy, Clone, PartialEq)]
struct ChunkDescriptor {
    /// The start offset of the chunk within the whole Stream
    offset: VarInt,
    /// The length of the chunk
    len: u32,
    /// the transmission state of the chunk
    state: ChunkTransmissionState,
    /// `true` if this is the final chunk, `false` otherwise
    fin: bool,
}

/// Potential transmission states for a chunk
#[derive(Debug, Copy, Clone, PartialEq)]
enum ChunkTransmissionState {
    /// The data had been enqueued, but is not currently being transmitted
    Enqueued,
    /// The data had been transmitted, but not yet acknowledged by the peer.
    InFlight(PacketNumber),
    /// The data had been acknowledged
    Acknowledged,
}

/// Enumerates states of the [`DataSender`]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DataSenderState {
    /// Outgoing data is accepted and transmitted
    Sending,
    /// The finish procedure has been initiated. New outgoing data is no longer
    /// accepted. The stream will continue to transmit data until all outgoing
    /// data has been transmitted and acknowledged successfully.
    /// In that case the `FinishAcknowledged` state will be entered.
    Finishing,
    /// All outgoing data including the FIN flag had been acknowledged.
    /// The Stream is thereby finalized.
    FinishAcknowledged,
    /// Sending data was cancelled due to a Stream RESET.
    Cancelled,
}

/// Manages the transmission of all `Stream` and `Crypto` data frames towards
/// the peer as long as the `Stream` has not been resetted or closed.
#[derive(Debug)]
pub struct DataSender<FlowControllerType, ChunkToFrameWriterType> {
    /// The data that needs to get transmitted
    data: VecDeque<Bytes>,
    /// Tracking information for all data in transmission
    tracking: VecDeque<ChunkDescriptor>,
    /// The number of currently enqueued bytes
    enqueued: u32,
    /// The number of chunks which are currently in-flight.
    /// This covers initially transmitted chunks as well as retransmitted ones.
    chunks_inflight: u32,
    /// The number of bytes which have already been acknowledged, but which
    /// are not yet released from the DataSender, since they are stuck behind
    /// other data which is not yet acknowledged
    acknowledged: u32,
    /// The number of chunks which are enqueued and not currently transmitted
    chunks_waiting_for_transmission: u32,
    /// The total amount of bytes which have been transmitted AND acknowledged.
    /// This is equivalent to the offset at the beginning of our send queue.
    total_acknowledged: VarInt,
    /// The maximum amount of bytes that are buffered within the sending stream.
    /// This capacity will not be exceeded - even if the remote provides us a
    /// bigger flow control window.
    max_buffer_capacity: u32,
    /// The flow controller which is used to determine whether data chunks can
    /// be sent.
    flow_controller: FlowControllerType,
    /// Whether the size of the send stream is known and a FIN flag is already
    /// enqueued.
    state: DataSenderState,
    /// Serializes chunks into frames and writes the frames
    writer: ChunkToFrameWriterType,
}

impl<
        FlowControllerType: OutgoingDataFlowController,
        ChunkToFrameWriterType: ChunkToFrameWriter,
    > DataSender<FlowControllerType, ChunkToFrameWriterType>
{
    /// The minimum payload size we want to be able to write in a single frame,
    /// in case the frame would get fragmented due to this.
    /// We want to avoid writing too small chunks, since every chunk requires us
    /// to allocate an associated tracking state on sender and receiver side.
    const MIN_WRITE_SIZE: usize = 32;

    /// Creates a new `DataSender` instance.
    ///
    /// `initial_window` denotes the amount of data we are allowed to send to the
    /// peer as known through transport parameters.
    /// `maximum_buffer_capacity` is the maximum amount of data the queue will
    /// hold. If users try to enqueue more data, it will be rejected in order to
    /// provide back-pressure on the `Stream`.
    pub fn new(flow_controller: FlowControllerType, max_buffer_capacity: u32) -> Self {
        Self {
            data: VecDeque::new(),
            tracking: VecDeque::new(),
            enqueued: 0,
            chunks_inflight: 0,
            acknowledged: 0,
            chunks_waiting_for_transmission: 0,
            total_acknowledged: VarInt::from_u8(0),
            flow_controller,
            max_buffer_capacity,
            state: DataSenderState::Sending,
            writer: ChunkToFrameWriterType::default(),
        }
    }

    /// Creates a new `DataSender` instance in its final
    /// [`DataSenderState::FinishAcknowledged`] state.
    pub fn new_finished(flow_controller: FlowControllerType, max_buffer_capacity: u32) -> Self {
        let mut result = Self::new(flow_controller, max_buffer_capacity);
        result.state = DataSenderState::FinishAcknowledged;
        result
    }

    /// Returns the flow controller for this `DataSender`
    pub fn flow_controller(&self) -> &FlowControllerType {
        &self.flow_controller
    }

    /// Returns the flow controller for this `DataSender`
    pub fn flow_controller_mut(&mut self) -> &mut FlowControllerType {
        &mut self.flow_controller
    }

    /// Stops sending out outgoing data.
    ///
    /// This is a one-way operation - sending can not be resumed.
    ///
    /// Calling the method removes all pending outgoing data as well as
    /// all tracking information from the buffer.
    pub fn stop_sending(&mut self) {
        if self.state == DataSenderState::FinishAcknowledged {
            return;
        }

        self.state = DataSenderState::Cancelled;
        self.flow_controller_mut().finish();
        self.data.clear();
        self.tracking.clear();
        self.enqueued = 0;
        self.chunks_inflight = 0;
        self.acknowledged = 0;
        self.chunks_waiting_for_transmission = 0;
    }

    /// Returns the amount of bytes that have ever been enqueued for writing on
    /// this Stream. This equals the offset of the highest enqueued byte + 1.
    pub fn total_enqueued_len(&self) -> VarInt {
        self.total_acknowledged + VarInt::from_u32(self.enqueued)
    }

    /// Returns the amount of bytes that are currently enqueued for sending on
    /// this Stream.
    pub fn enqueued_len(&self) -> usize {
        self.enqueued as usize
    }

    /// Returns the state of the sender
    pub fn state(&self) -> DataSenderState {
        self.state
    }

    /// Overwrites the amount of total received and acknowledged bytes.
    ///
    /// This method is only used for testing purposes, in order to simulate a
    /// large number of already received bytes. The value is normally updated
    /// as an implementation detail!
    #[cfg(test)]
    pub fn set_total_acknowledged_len(&mut self, total_acknowledged: VarInt) {
        assert_eq!(
            VarInt::from_u8(0),
            self.total_enqueued_len(),
            "set_total_acknowledged_len can only be called on a new stream"
        );
        self.total_acknowledged = total_acknowledged;
    }

    /// Returns the amount of data that can be additionally buffered for sending
    ///
    /// This depends on the configured maximum buffer size.
    /// We do not utilize the window size that the peer provides us in order to
    /// avoid excessive buffering in case the peer would provide a very big window.
    pub fn available_buffer_space(&self) -> usize {
        // We can admit more data than the maximum buffer capacity temporarily
        if self.enqueued >= self.max_buffer_capacity {
            return 0;
        }
        let available_buffer_window = self.max_buffer_capacity - self.enqueued;
        available_buffer_window as usize
    }

    /// Enqueues the data for transmission.
    ///
    /// It is only allowed to enqueue bytes if they do not overflow the maximum
    /// allowed Stream window (of the maximum VarInt size). This is not checked
    /// inside this method. The already enqueued bytes can be retrieved by
    /// calling [`total_enqueued_len()`].
    pub fn push(&mut self, data: Bytes) {
        debug_assert!(
            self.state == DataSenderState::Sending,
            "Data transmission is not allowed after finish() was called"
        );
        debug_assert!(
            data.len() <= core::u32::MAX as usize,
            "Maximum data size exceeded"
        );

        let data_len = data.len() as u32;
        self.enqueued += data_len;
        self.chunks_waiting_for_transmission += 1;
        self.data.push_back(data);

        // Add tracking state. We can not merge the tracking state with the one
        // of the last pending buffer currently, because some logic here requires
        // that each `ChunkDescriptor` only tracks exactly one `Bytes` buffer.
        // If we merge multiple tracking states, then the resulting merged state
        // would refer to multiple buffers.
        let offset = if let Some(ChunkDescriptor { offset, len, .. }) = self.tracking.back() {
            *offset + VarInt::from_u32(*len)
        } else {
            self.total_acknowledged
        };

        self.tracking.push_back(ChunkDescriptor {
            state: ChunkTransmissionState::Enqueued,
            len: data_len,
            offset,
            fin: false,
        });
    }

    /// Starts the finalization process of a `Stream` by enqueuing a `FIN` frame.
    pub fn finish(&mut self) {
        if self.state != DataSenderState::Sending {
            return;
        }
        self.state = DataSenderState::Finishing;

        let offset = if let Some(last_chunk) = self.tracking.back_mut() {
            if last_chunk.state == ChunkTransmissionState::Enqueued {
                // The last chunk is currently not submitted we can piggyback
                // the FIN flag on it
                last_chunk.fin = true;
                return;
            }
            last_chunk.offset + VarInt::from_u32(last_chunk.len)
        } else {
            self.total_acknowledged
        };

        self.tracking.push_back(ChunkDescriptor {
            state: ChunkTransmissionState::Enqueued,
            len: 0,
            offset,
            fin: true,
        });
        self.chunks_waiting_for_transmission += 1;
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        // This flag is just an optimization. If we do not get acknowledgements
        // for data at the head of the queue, we not need to dequeue data.
        let mut check_released = false;

        for (index, chunk) in self.tracking.iter_mut().enumerate() {
            if let ChunkTransmissionState::InFlight(inflight_packet_nr) = chunk.state {
                if ack_set.contains(inflight_packet_nr) {
                    // The chunk was acknowledged
                    chunk.state = ChunkTransmissionState::Acknowledged;
                    self.chunks_inflight -= 1;
                    self.acknowledged += chunk.len;

                    if index == 0 {
                        check_released = true;
                    }
                    // No `break` here!
                    // The same packet number can be used by multiple packets
                }
            }
        }

        if !check_released {
            return;
        }

        // Remove all segments which have been fully acknowledged from the
        // front of the list and record how many bytes have been fully transmitted.
        let mut released = 0;
        while !self.tracking.is_empty() {
            if let Some(first) = self.tracking.front() {
                if first.state == ChunkTransmissionState::Acknowledged {
                    released += first.len;
                    self.acknowledged -= first.len;
                    self.tracking.pop_front();
                } else {
                    break;
                }
            }
        }

        self.enqueued -= released;
        self.total_acknowledged += VarInt::from_u32(released);

        // Release the associated byte segments
        // Convert to usize, because the `Bytes` segments use this unit
        let mut released = released as usize;
        while released > 0 {
            let buffer = self.data.front_mut().expect("Buffer must be available");
            let to_release = core::cmp::min(released, buffer.len());
            if buffer.len() <= released {
                // The buffer had been fully consumed and can be released
                self.data.pop_front();
            } else {
                // A part of the buffer had been released.
                // Slice the remaining buffer.
                buffer.advance(released);
            }
            released -= to_release;
        }

        // If the FIN was enqueued, and all outgoing data had been transmitted,
        // then we have finalized the stream.
        if self.tracking.is_empty() && self.state == DataSenderState::Finishing {
            self.state = DataSenderState::FinishAcknowledged;
            self.flow_controller_mut().finish();
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        for chunk in self.tracking.iter_mut() {
            if let ChunkTransmissionState::InFlight(inflight_packet_nr) = chunk.state {
                if ack_set.contains(inflight_packet_nr) {
                    // If the chunk was lost, mark it as Enqueued again, so that
                    // it will get sent again.
                    //
                    // Potential TODO here: Merge adjacent `Enqueued` states,
                    // so that we do not waste as much memory for tracking states
                    // if lots of packets are lost.
                    // This however will likely only be relevant, if we also
                    // - shrink the `tracking` list if it gets smaller in order
                    //   actually release memory.
                    // - provide a set of packet numbers to `on_packet_loss` and
                    //   `on_packet_ack`. That way we would actually would change
                    //   more adjacent blocks, which can then be merged.
                    // This will also only work if the chunks are derived from the
                    // same `Bytes` buffer or if we later on implement sending
                    // chunks from multiple `Bytes` buffers for a single chunk.
                    chunk.state = ChunkTransmissionState::Enqueued;
                    self.chunks_inflight -= 1;
                    self.chunks_waiting_for_transmission += 1;

                    // Since a packet needs to get retransmitted, we are
                    // no longer blocked on waiting for flow control windows
                    self.flow_controller.clear_blocked();

                    // More than 1 chunk can use the same Packet number, if
                    // multiple chunks are written into the same packet.
                    // Therefore we can not `break` here.
                    //
                    // We can also not break if we observe a higher packet number,
                    // since a packet loss might have caused an old segment
                    // (earlier in the list) to be retransmitted with a higher
                    // packet number. In that case chunks later in the list which
                    // have not yet been retransmitted will use lower numbers.
                }
            }
        }
    }

    /// Returns the content of the byte buffer which starts at a given
    /// offset. It is only allowed to call the method for offsets which are
    /// tracked by the the `DataSender` in an associated `ChunkDescriptor` struct, and
    /// which are thereby not out of bounds.
    fn bytes_at_offset(
        buffers: &VecDeque<Bytes>,
        first_buffer_offset: VarInt,
        offset: VarInt,
    ) -> &[u8] {
        let mut current_offset = first_buffer_offset;
        for buffer in buffers.iter() {
            if offset >= current_offset && offset < (current_offset + buffer.len()) {
                let buffer_offset = Into::<u64>::into(offset - current_offset) as usize;
                return &buffer[buffer_offset..];
            }
            current_offset += buffer.len();
        }

        // The following branch will be entered for finding the associated byte
        // segment for an empty range - which is only utilized to transmit the
        // FIN flag. In that case the start offset of the range will line up
        // with the end of all other enqueued byte buffers.
        if offset == current_offset {
            return &[];
        }

        unreachable!("Did not find associated buffer");
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: ChunkToFrameWriterType::StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        let mut chunk_index = 0;
        while chunk_index < self.tracking.len() {
            let chunk = &mut self.tracking[chunk_index];

            if let ChunkTransmissionState::Enqueued = chunk.state {
                // We are not allowed to write more than the flow control window.
                // However it is possible that more data gets enqueued. In order
                // to make sure we do not violate flow control, chunks are truncated
                // to the maximum window.

                let chunk_end = chunk.offset + VarInt::from_u32(chunk.len);
                let window_end = self
                    .flow_controller
                    .acquire_flow_control_window(chunk.offset, chunk.len as usize);
                let truncate_by = Into::<u64>::into(chunk_end.saturating_sub(window_end)) as usize;
                if truncate_by == (chunk.len as usize) && chunk.len > 0 {
                    // Can not write anything in this chunk due to being beyond
                    // the flow control window. In this case we can break.
                    // This can happen in the very first loop iteration in case
                    // `on_transmit` is called with no other outstanding data,
                    // but also when we truncated the previous chunk and still
                    // continue to iterate.
                    //
                    // Chunks are ordered by offset, so we will not be able to
                    // write a later offset and thereby won't miss anything
                    // by breaking early.

                    // TODO: This might be a place where we could send a
                    // STREAM_DATA_BLOCKED frame later on. If we are doing this,
                    // we should make sure that the frame gets emitted only once
                    // for a given blocked offset. E.g. we could store an
                    // `Option` variable somewhere that tracks whether we already
                    // have enqueued such a frame.
                    break;
                }
                // This is the maximum size we intend to send after being limited
                // to the flow control window.
                let send_size = chunk.len as usize - truncate_by;

                let available_space = context
                    .reserve_minimum_space_for_frame(self.writer.get_max_frame_size(
                        stream_id,
                        core::cmp::min(Self::MIN_WRITE_SIZE, send_size),
                    ))
                    .map_err(|_| OnTransmitError::CoundNotAcquireEnoughSpace)?;

                // Check how much data we can fit into that amount of space,
                // given our other known variables. The amount of possible
                // payload is different between whether this is the last frame
                // in a packet or another frame, since we do not have to write
                // length information in the last frame. That allows to send us
                // 1-4 extra bytes (realistically 2, since UDP packets are not
                // that big).
                let max_payload_sizes =
                    self.writer
                        .max_payload_size(stream_id, available_space, chunk.offset);

                let (send_size, is_last_frame) =
                    if send_size > max_payload_sizes.max_payload_in_all_frames {
                        // The chunk would max out the payload size for frames with
                        // a length field. Therefore we will write it without one.
                        //
                        // Note that we still need a `min` expression here, since the chunk
                        // might not fit into frames with length field, but it might still
                        // be smaller than the space available in the last frame.
                        (
                            core::cmp::min(send_size, max_payload_sizes.max_payload_as_last_frame)
                                as u32,
                            true,
                        )
                    } else {
                        // The payload is smaller or equal than the allowed payload
                        // size in frames with length field. Therefore we can write
                        // it that way.
                        (send_size as u32, false)
                    };

                // We should at least be able to write some bytes, due to requesting
                // a minimal amount of space. And expect for FINs, we do not have
                // 0 byte chunks.
                debug_assert!(
                    send_size > 0 || chunk.fin,
                    "Expected to be able to write data"
                );

                // Find the payload
                let payload_bytes =
                    Self::bytes_at_offset(&self.data, self.total_acknowledged, chunk.offset);

                let packet_nr = self
                    .writer
                    .write_value_as_frame(
                        stream_id,
                        chunk.offset,
                        &payload_bytes[..send_size as usize],
                        is_last_frame,
                        // If not all data in the chunk can be written, the chunk
                        // gets split and the next chunk will carry the FIN
                        chunk.fin && send_size == chunk.len,
                        context,
                    )
                    .ok_or(OnTransmitError::CouldNotWriteFrame)?;

                self.chunks_inflight += 1;
                chunk.state = ChunkTransmissionState::InFlight(packet_nr);

                if send_size != chunk.len {
                    // Only a part of this chunk could be written into the frame.
                    // In this case we need to create a new ChunkDescriptor for the remaining
                    // parts.
                    let new_range = ChunkDescriptor {
                        offset: chunk.offset + VarInt::from_u32(send_size),
                        len: chunk.len - send_size,
                        state: ChunkTransmissionState::Enqueued,
                        fin: chunk.fin,
                    };
                    chunk.len = send_size;
                    self.tracking.insert(chunk_index + 1, new_range);
                } else {
                    self.chunks_waiting_for_transmission -= 1;
                }
            }

            chunk_index += 1;
        }

        Ok(())
    }
}

impl<F: OutgoingDataFlowController, S> FrameExchangeInterestProvider for DataSender<F, S> {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests {
            delivery_notifications: self.chunks_inflight > 0,
            transmission: self.chunks_waiting_for_transmission > 0
                && !self.flow_controller.is_blocked(),
            ignore_congestion_control: false,
        }
    }
}
