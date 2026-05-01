// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Batch structure for reliable datagram transmission

use crate::{
    clock::{precision, wheel::SingleTimer},
    intrusive_queue::{Entry, Queue},
    packet::datagram::partial::PartialDatagram,
    socket::pool::descriptor,
};
use s2n_quic_core::varint::VarInt;
use std::net::SocketAddr;

/// A batch of partial datagrams ready for transmission
///
/// Flows through the instance-wide wheel, sorted by transmission time.
/// All datagrams in a batch must go to the same peer (enforced at construction).
///
/// # Encoding Pipeline
/// 1. Application creates PartialDatagrams -> Batch
/// 2. Batch goes through wheel (timing)
/// 3. Batch distributed to socket workers
/// 4. **Encoder step**: Converts PartialDatagrams into encoded bytes
///    - Allocates packet numbers per datagram
///    - Gets sealer/credentials from path_secret_entry
///    - Encodes into GSO segments in storage buffer
/// 5. **Send step**: Transmits encoded bytes from storage
pub struct Batch {
    /// Intrusive queue of partial datagrams
    pub datagrams: Queue<PartialDatagram>,
    /// Transmission time (1us intervals per sender)
    pub transmission_time: Option<precision::Timestamp>,
    /// Metadata for socket workers
    pub meta: Meta,
    /// Storage field for fully-encoded packets with GSO segment size
    pub encoded: Option<descriptor::Segments>,
}

impl Batch {
    /// Creates a new batch with the given transmission time
    #[inline]
    pub fn new(transmission_time: Option<precision::Timestamp>, peer_addr: SocketAddr) -> Self {
        Self {
            datagrams: Queue::new(),
            transmission_time,
            meta: Meta {
                total_bytes: 0,
                peer_addr,
                starting_packet_number: None,
                is_probe: false,
            },
            encoded: None,
        }
    }

    /// Pushes a datagram into this batch
    ///
    /// Updates total_bytes metadata.
    #[inline]
    pub fn push(&mut self, datagram: Entry<PartialDatagram>) {
        let len = datagram.estimate_encoded_len(16);
        // TODO assert it fits into u16
        // TODO assert the total_len doesn't overflow
        // TODO assert that the len is less than or equal to the existing datagrams, if any
        self.meta.total_bytes += len as u16;
        self.datagrams.push_back(datagram);
    }

    /// Returns true if the batch is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.datagrams.is_empty()
    }

    /// Returns the number of datagrams in the batch
    #[inline]
    pub fn len(&self) -> usize {
        self.datagrams.len()
    }
}

/// Builder for constructing batches with GSO constraints
///
/// Maintains state to enforce uniform segment sizes and other GSO requirements.
pub struct Builder {
    /// The batch being built
    batch: Batch,
    /// The uniform segment size (all segments except last must match this)
    segment_size: Option<u16>,
    /// Whether the last segment added was undersized (smaller than uniform size)
    /// Once true, no more segments can be added (GSO requires last segment to be final)
    has_undersized_segment: bool,
}

impl Builder {
    /// Creates a new batch builder
    #[inline]
    pub fn new(transmission_time: Option<precision::Timestamp>, peer_addr: SocketAddr) -> Self {
        Self {
            batch: Batch::new(transmission_time, peer_addr),
            segment_size: None,
            has_undersized_segment: false,
        }
    }

    /// Tries to push a datagram into this batch, checking GSO constraints
    ///
    /// Returns `Ok(())` if the datagram was added, or `Err(datagram)` if it couldn't
    /// be added due to batch constraints.
    ///
    /// # Constraints checked:
    /// - Maximum segment count (GSO limit)
    /// - Maximum total payload size (sendmsg limit)
    /// - Uniform segment size (GSO requires all segments same size except last)
    /// - No segments after an undersized segment (GSO requires last segment to be final)
    /// - Destination address match
    pub fn try_push(
        &mut self,
        datagram: Entry<PartialDatagram>,
    ) -> Result<(), Entry<PartialDatagram>> {
        use crate::msg::segment;

        // If we already added an undersized segment, we can't add more
        // GSO requires the undersized segment to be the final one
        if self.has_undersized_segment {
            return Err(datagram);
        }

        let len = datagram.estimate_encoded_len(16);

        // Check if we've hit the maximum segment count
        let current_count = self.batch.datagrams.len();
        if current_count >= segment::MAX_COUNT {
            return Err(datagram);
        }

        // Check if adding this would exceed the maximum total payload size
        let new_total = self.batch.meta.total_bytes as u32 + len as u32;
        if new_total > segment::MAX_TOTAL as u32 {
            return Err(datagram);
        }

        // Check destination address matches
        if datagram.remote_address() != self.batch.meta.peer_addr {
            return Err(datagram);
        }

        // GSO requires uniform segment sizes (except the last segment can be smaller)
        if let Some(expected_size) = self.segment_size {
            // We already have a uniform size established
            // New segment must either match it or be smaller (final segment)
            if len > expected_size as usize {
                return Err(datagram);
            }
            // Mark if this segment is undersized
            if len < expected_size as usize {
                self.has_undersized_segment = true;
            }
        } else {
            // This is the first segment, establish the uniform size
            // Clamp to u16 since segment sizes must fit in u16
            self.segment_size = Some(len.min(u16::MAX as usize) as u16);
        }

        // All constraints satisfied, add to batch
        self.batch.meta.total_bytes += len as u16;
        self.batch.datagrams.push_back(datagram);
        Ok(())
    }

    /// Finishes building and returns the batch
    #[inline]
    pub fn finish(self) -> Batch {
        self.batch
    }

    /// Returns true if the batch is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Returns the number of datagrams in the batch
    #[inline]
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Returns the current batch metadata
    #[inline]
    pub fn meta(&self) -> &Meta {
        &self.batch.meta
    }
}

impl SingleTimer for Batch {
    #[inline]
    fn target_time(&self) -> Option<precision::Timestamp> {
        self.transmission_time
    }

    #[inline]
    fn set_target_time(&mut self, time: precision::Timestamp) {
        self.transmission_time = Some(time);
    }
}

impl crate::socket::channel::ByteCost for Batch {
    fn byte_cost(&self) -> u64 {
        self.meta.total_bytes as u64
    }
}

// TODO: Implement encoding pipeline for Batch
// Steps needed:
// 1. Create encoder adapter in socket pipeline (before Sendable)
// 2. Encoder allocates packet numbers per datagram (per-peer counter)
// 3. Gets sealer/credentials from path_secret_entry
// 4. Encodes each PartialDatagram into GSO segments
// 5. Stores encoded bytes in Batch.encoded field
// 6. Sendable impl just transmits from Batch.encoded storage

impl crate::socket::channel::Sendable for Batch {
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()> {
        self.encoded
            .as_mut()
            .expect("batch must be encoded before sending")
            .send(socket)
    }
}

/// Metadata about a batch for socket workers
#[derive(Clone)]
pub struct Meta {
    /// Total bytes in all datagrams (for rate limiting)
    pub total_bytes: u16,
    /// Destination peer address
    pub peer_addr: SocketAddr,
    /// Starting packet number for this batch (datagrams numbered contiguously)
    pub starting_packet_number: Option<VarInt>,
    /// Whether this batch is a probe (skips packet numbers to elicit immediate ACK)
    pub is_probe: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{byte_vec::ByteVec, packet::RoutingInfo, path::secret::map::Entry};

    #[test]
    fn batch_creation() {
        let batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.meta.total_bytes, 0);
    }

    #[test]
    fn batch_push() {
        let mut batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        let entry = Entry::fake("127.0.0.1:8080".parse().unwrap(), None);

        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"test"));

        let datagram = PartialDatagram::new_datagram(
            RoutingInfo::None,
            ByteVec::new(),
            payload,
            entry,
            None.into(),
        );

        batch.push(datagram.into());
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.meta.total_bytes, 4); // payload only
    }

    #[test]
    fn batch_scheduled() {
        let mut batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(batch.target_time(), None);

        let time = precision::Timestamp { nanos: 1000000 }; // 1ms in nanos
        batch.set_target_time(time);
        assert_eq!(batch.target_time(), Some(time));
    }
}
