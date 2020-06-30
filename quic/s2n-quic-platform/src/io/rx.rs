use s2n_quic_core::{inet::DatagramInfo, time::Timestamp};

/// Abstraction over receiving datagram from the network
pub trait RxQueue {
    /// Pops a datagram from the queue
    fn pop(&mut self, timestamp: Timestamp) -> Option<(DatagramInfo, &mut [u8])>;

    /// Returns the number of pending datagrams
    fn len(&self) -> usize;

    /// Returns `true` if the queue has no datagrams
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
