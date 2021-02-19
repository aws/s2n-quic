use super::View;
use crate::contexts::WriteContext;
use s2n_quic_core::{frame::FitError, varint::VarInt};

/// Manages the outgoing flow control window for a sending data on a particular
/// data stream.
pub trait OutgoingDataFlowController {
    /// Tries to acquire a flow control window for the described chunk of data.
    /// The implementation must return the **maximum** (exclusive) offset up to
    /// which the data sender is allowed to send.
    fn acquire_flow_control_window(&mut self, end_offset: VarInt) -> VarInt;

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
pub trait FrameWriter: Default {
    // A value to be passed to the frame writer
    type Context: Copy;

    const WRITES_FIN: bool = true;

    /// The minimum payload size we want to be able to write in a single frame,
    /// in case the frame would get fragmented due to this.
    /// We want to avoid writing too small chunks, since every chunk requires us
    /// to allocate an associated tracking state on sender and receiver side.
    const MIN_WRITE_SIZE: usize = 32;

    fn write_chunk<W: WriteContext>(
        &self,
        offset: VarInt,
        payload: &mut View,
        writer_context: Self::Context,
        context: &mut W,
    ) -> Result<(), FitError>;

    fn write_fin<W: WriteContext>(
        &self,
        offset: VarInt,
        writer_context: Self::Context,
        context: &mut W,
    ) -> Result<(), FitError>;
}
