use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
pub use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

/// Abstraction over sending datagrams over the network
pub trait TxQueue {
    /// Pushes the `Payload` onto the queue.
    ///
    /// Returns `Ok(message_len)` if successful, otherwise
    /// `Err(TxError)`.
    fn push<Payload: TxPayload>(
        &mut self,
        remote_address: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Payload,
    ) -> Result<usize, TxError>;

    /// Pushes the `Payload` that implements `EncoderValue`
    /// onto the queue.
    ///
    /// Returns `Ok(message_len)` if successful, otherwise
    /// `Err(TxError)`.
    fn push_encoder_value<Payload: EncoderValue>(
        &mut self,
        remote_address: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Payload,
    ) -> Result<usize, TxError> {
        self.push(remote_address, ecn, EncoderValuePayload(payload))
    }

    /// Returns the number of remaining datagrams that can be transmitted
    fn capacity(&self) -> usize;

    /// Returns `true` if the queue will accept additional transmissions
    fn can_push(&self) -> bool {
        self.capacity() != 0
    }

    /// Returns the number of pending datagrams to be transmitted
    fn len(&self) -> usize;

    /// Returns `true` if there are no pending datagrams to be transmitted
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TxError {
    /// The queue cannot accept transmissions at the current time
    AtCapacity,

    /// The message was cancelled by trying to send an empty payload
    Cancelled,
}

/// Trait for writing transmission payloads into a supplied buffer
pub trait TxPayload {
    fn write(self, buffer: &mut [u8]) -> usize;
}

/// Implemented for convenience
impl<F: FnOnce(&mut [u8]) -> usize> TxPayload for F {
    fn write(self, buffer: &mut [u8]) -> usize {
        (self)(buffer)
    }
}

/// New type for payloads that implement `EncoderValue`
pub struct EncoderValuePayload<T: EncoderValue>(pub T);

impl<'a, T: EncoderValue> TxPayload for EncoderValuePayload<T> {
    fn write(mut self, buffer: &mut [u8]) -> usize {
        let mut buffer = EncoderBuffer::new(buffer);
        self.0.encode_mut(&mut buffer);
        buffer.len()
    }
}
