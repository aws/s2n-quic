use crate::contexts::{ConnectionContext, WriteContext};
use alloc::collections::VecDeque;
use s2n_codec::{encoder::EncoderBuffer, DecoderBufferMut};
use s2n_quic_core::{
    connection::ConnectionId,
    endpoint::EndpointType,
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        FrameMut,
    },
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    varint::VarInt,
};

/// A [`ConnectionContext`] that is used for testing
#[derive(Clone, Debug)]
pub struct MockConnectionContext {
    pub local_endpoint_type: EndpointType,
    connection_id: ConnectionId,
}

impl MockConnectionContext {
    pub fn new(local_endpoint_type: EndpointType) -> MockConnectionContext {
        MockConnectionContext {
            local_endpoint_type,
            connection_id: ConnectionId::try_from_bytes(&[]).unwrap(),
        }
    }
}

impl ConnectionContext for MockConnectionContext {
    fn local_endpoint_type(&self) -> EndpointType {
        self.local_endpoint_type
    }

    fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }
}

/// A single frame which had been written.
/// The buffer always stores only a single serialized Frame.
#[derive(Clone, Debug)]
pub struct WrittenFrame {
    pub data: Vec<u8>,
    pub packet_nr: PacketNumber,
}

impl WrittenFrame {
    /// Deserializes a written frame into a quic_frame::Frame type.
    /// panics if deserialization fails.
    pub fn as_frame(&mut self) -> FrameMut {
        let buffer = DecoderBufferMut::new(&mut self.data[..]);
        let (frame, remaining) = buffer
            .decode::<FrameMut>()
            .expect("Buffer contains a valid frame");
        assert_eq!(0, remaining.len());
        frame
    }
}

/// Stores frames which have been written by components
#[derive(Clone, Debug)]
pub struct OutgoingFrameBuffer {
    ack_elicitation: AckElicitation,
    /// Frames which had been written so far
    frames: VecDeque<WrittenFrame>,
    /// The PacketNumber which will be returned by the next `write_frame()` call
    next_packet_nr: PacketNumber,
    /// If set, this indicates the maximum packet size
    max_buffer_size: Option<usize>,
    /// The remaining space in the current packet
    remaining_packet_space: usize,
    /// If set, the value indicates after how many `write_frame()` will still be
    /// permitted before errors are returned on write. This can be used to simulate
    /// failing write calls.
    error_after_frames: Option<usize>,
}

impl Default for OutgoingFrameBuffer {
    fn default() -> Self {
        OutgoingFrameBuffer {
            ack_elicitation: Default::default(),
            frames: VecDeque::new(),
            next_packet_nr: PacketNumberSpace::ApplicationData
                .new_packet_number(VarInt::from_u8(0)),
            max_buffer_size: None,
            remaining_packet_space: 0,
            error_after_frames: None,
        }
    }
}

impl OutgoingFrameBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all frames which have been fully written
    pub fn clear(&mut self) {
        self.frames.clear();
        self.ack_elicitation = Default::default();
    }

    /// Configures an explicit maximum packet size. If this is configured the
    /// component will write multiple frames into a single packet.
    /// Otherwise an individual packet will get written for each frame.
    pub fn set_max_packet_size(&mut self, max_buffer_size: Option<usize>) {
        // If we go from one mode to another, we should flush any pending
        // packets in between.
        self.flush();
        self.max_buffer_size = max_buffer_size;

        if let Some(max_buffer_size) = max_buffer_size {
            self.remaining_packet_space = max_buffer_size;
        }
    }

    /// Instructs the `FrameBuffer` to only allow `n` frame writes,
    /// and to fail the following write attempt
    pub fn set_error_write_after_n_frames(&mut self, n: usize) {
        self.error_after_frames = Some(n);
    }

    /// Returns the amount of written frames
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns false if there are written frames
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns the oldest written frame
    pub fn pop_front(&mut self) -> Option<WrittenFrame> {
        self.frames.pop_front()
    }

    /// Flushes any pending packet
    pub fn flush(&mut self) {
        if let Some(max_buffer_size) = self.max_buffer_size {
            if self.remaining_packet_space == max_buffer_size {
                // No frame had been written into this packet
                return;
            }

            // Increment the packet number and reset the amount of available
            // packet space. Thereby frames after that flush will get assigned
            // a fresh packet number.
            self.next_packet_nr = self.next_packet_nr.next().unwrap();
            self.remaining_packet_space = max_buffer_size;
            self.ack_elicitation = Default::default();
        }
    }

    fn reserve_minimum_space_for_frame(&mut self, min_size: usize) -> Result<usize, ()> {
        if let Some(max_buffer_size) = self.max_buffer_size {
            if min_size > max_buffer_size {
                return Err(());
            }

            if min_size >= self.remaining_packet_space {
                self.flush();
            }

            Ok(self.remaining_packet_space)
        } else {
            Ok(core::usize::MAX)
        }
    }

    fn encode_frame_to_vec<Frame: s2n_codec::EncoderValue>(
        frame: &Frame,
        encoded_size: usize,
    ) -> Vec<u8> {
        // Create a new buffer for the frame
        let mut write_buffer = vec![0u8; encoded_size];
        let mut encoder_buffer = EncoderBuffer::new(&mut write_buffer[..]);
        frame.encode(&mut encoder_buffer);
        write_buffer
    }

    pub fn write_frame<Frame: s2n_codec::EncoderValue + AckElicitable>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        if let Some(error_after_frames) = self.error_after_frames {
            if error_after_frames == 0 {
                // No more frames are permitted -> fail the write
                return None;
            }
            // Decrease the amount of permitted writes
            self.error_after_frames = Some(error_after_frames - 1);
        }

        let encoded_size = frame.encoding_size();

        if let Some(max_buffer_size) = self.max_buffer_size {
            if encoded_size > max_buffer_size {
                // This can never be written
                return None;
            }

            if self.remaining_packet_space < encoded_size {
                // Flush the current packet to make space
                self.flush();
            }

            let encoded_frame = Self::encode_frame_to_vec(frame, encoded_size);

            // Store the frame, but don't increase the packet number, since we
            // might be writing more frames after this.
            let packet_nr = self.next_packet_nr;

            self.ack_elicitation |= frame.ack_elicitation();
            self.frames.push_back(WrittenFrame {
                data: encoded_frame,
                packet_nr,
            });

            self.remaining_packet_space -= encoded_size;

            Some(self.next_packet_nr)
        } else {
            // There is no write limit configured. This means we directly store
            // each frame as an individual buffer.
            let encoded_frame = Self::encode_frame_to_vec(frame, encoded_size);

            let packet_nr = self.next_packet_nr;
            self.next_packet_nr = self.next_packet_nr.next().unwrap();

            self.ack_elicitation |= frame.ack_elicitation();
            self.frames.push_back(WrittenFrame {
                data: encoded_frame,
                packet_nr,
            });

            Some(packet_nr)
        }
    }
}

#[derive(Debug)]
pub struct MockWriteContext<'a> {
    connection_context: &'a MockConnectionContext,
    current_time: Timestamp,
    frame_buffer: &'a mut OutgoingFrameBuffer,
}

impl<'a> MockWriteContext<'a> {
    pub fn new(
        connection_context: &'a MockConnectionContext,
        current_time: Timestamp,
        frame_buffer: &'a mut OutgoingFrameBuffer,
    ) -> MockWriteContext<'a> {
        MockWriteContext {
            connection_context,
            current_time,
            frame_buffer,
        }
    }
}

impl<'a> WriteContext for MockWriteContext<'a> {
    type ConnectionContext = MockConnectionContext;

    fn current_time(&self) -> Timestamp {
        self.current_time
    }

    fn connection_context(&self) -> &Self::ConnectionContext {
        self.connection_context
    }

    fn write_frame<Frame: s2n_codec::EncoderValue + AckElicitable>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        self.frame_buffer.write_frame(frame)
    }

    fn ack_elicitation(&self) -> AckElicitation {
        self.frame_buffer.ack_elicitation
    }

    fn packet_number(&self) -> PacketNumber {
        self.frame_buffer.next_packet_nr
    }

    fn reserve_minimum_space_for_frame(&mut self, min_size: usize) -> Result<usize, ()> {
        self.frame_buffer.reserve_minimum_space_for_frame(min_size)
    }
}
