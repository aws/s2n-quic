// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Types and utilities around the QUIC Stream identifier

use crate::{endpoint, stream::StreamType, varint::VarInt};
#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

/// The ID of a stream.
///
/// A stream ID is a 62-bit integer (0 to 2^62-1) that is unique for all streams
/// on a connection.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone, Hash)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
pub struct StreamId(VarInt);

// Stream IDs can be converted into `VarInt` and `u64`

impl From<StreamId> for VarInt {
    fn from(id: StreamId) -> Self {
        id.0
    }
}

impl From<StreamId> for u64 {
    fn from(id: StreamId) -> Self {
        id.0.as_u64()
    }
}

impl StreamId {
    /// Creates a Stream ID from a [`VarInt`].
    ///
    /// This is always a safe conversion, since Stream IDs and [`VarInt`]s
    /// share the same range.
    #[inline]
    pub const fn from_varint(id: VarInt) -> StreamId {
        StreamId(id)
    }

    /// Converts the stream id into a [`VarInt`]
    #[inline]
    pub const fn as_varint(self) -> VarInt {
        self.0
    }

    /// Returns the initial Stream ID for a given stream type.
    ///
    /// E.g. the initial Stream ID for a server initiated unidirectional Stream
    /// is Stream ID `3`.
    ///
    /// Example:
    ///
    /// ```
    /// # use s2n_quic_core::{endpoint, stream::{StreamId, StreamType}};
    /// let stream_id = StreamId::initial(endpoint::Type::Server, StreamType::Unidirectional);
    /// // Initial server initiated unidirectional Stream ID is 3
    /// assert_eq!(3u64, stream_id.as_varint().as_u64());
    /// ```
    #[inline]
    pub fn initial(initiator: endpoint::Type, stream_type: StreamType) -> StreamId {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-2.1
        //# The two least significant bits from a stream ID therefore identify a
        //# stream as one of four types, as summarized in Table 1.
        //#
        //#        +======+==================================+
        //#        | Bits | Stream Type                      |
        //#        +======+==================================+
        //#        | 0x00 | Client-Initiated, Bidirectional  |
        //#        +------+----------------------------------+
        //#        | 0x01 | Server-Initiated, Bidirectional  |
        //#        +------+----------------------------------+
        //#        | 0x02 | Client-Initiated, Unidirectional |
        //#        +------+----------------------------------+
        //#        | 0x03 | Server-Initiated, Unidirectional |
        //#        +------+----------------------------------+

        match (
            stream_type == StreamType::Bidirectional,
            initiator == endpoint::Type::Client,
        ) {
            (true, true) => StreamId(VarInt::from_u32(0)),
            (true, false) => StreamId(VarInt::from_u32(1)),
            (false, true) => StreamId(VarInt::from_u32(2)),
            (false, false) => StreamId(VarInt::from_u32(3)),
        }
    }

    /// Returns the n-th `StreamId` for a certain type of `Stream`.
    ///
    /// The 0th `StreamId` thereby represents the `StreamId` which is returned
    /// by the [`Self::initial`] method. All further `StreamId`s of a certain type
    /// will be spaced apart by 4.
    ///
    /// nth() will return `None` if the resulting `StreamId` would not be valid.
    #[inline]
    pub fn nth(initiator: endpoint::Type, stream_type: StreamType, n: u64) -> Option<StreamId> {
        let initial = Self::initial(initiator, stream_type);
        // We calculate as much as possible with u64, to reduce the number of
        // overflow checks for the maximum Stream ID to the last operation
        let id = VarInt::new(n.checked_mul(4)?.checked_add(initial.into())?).ok()?;
        Some(StreamId(id))
    }

    /// Returns the next [`StreamId`] which is of the same type the one referred
    /// to. E.g. if the method is called on a Stream ID for an unidirectional
    /// client initiated stream, the Stream ID of the next unidirectional client
    /// initiated stream will be returned.
    ///
    /// Returns `None` if the next Stream ID would not be valid, due to being out
    /// of bounds.
    ///
    /// Example:
    ///
    /// ```
    /// # use s2n_quic_core::{endpoint, stream::{StreamId, StreamType}};
    /// let stream_id = StreamId::initial(endpoint::Type::Client, StreamType::Unidirectional);
    /// // Initial client initiated unidirectional Stream ID is 2
    /// assert_eq!(2u64, stream_id.as_varint().as_u64());
    /// // Get the next client initiated Stream ID
    /// let next_stream_id = stream_id.next_of_type();
    /// assert_eq!(6u64, next_stream_id.expect("Next Stream ID is valid").as_varint().as_u64());
    /// ```
    #[inline]
    pub fn next_of_type(self) -> Option<StreamId> {
        // Stream IDs increase in steps of 4, since the 2 least significant bytes
        // are used to indicate the stream type
        self.0
            .checked_add(VarInt::from_u32(4))
            .map(StreamId::from_varint)
    }

    /// Returns whether the client or server initiated the Stream
    #[inline]
    pub fn initiator(self) -> endpoint::Type {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-2.1
        //# The least significant bit (0x1) of the stream ID identifies the
        //# initiator of the stream.  Client-initiated streams have even-numbered
        //# stream IDs (with the bit set to 0)
        if Into::<u64>::into(self.0) & 0x01u64 == 0 {
            endpoint::Type::Client
        } else {
            endpoint::Type::Server
        }
    }

    /// Returns whether the Stream is unidirectional or bidirectional.
    #[inline]
    pub fn stream_type(self) -> StreamType {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-2.1
        //# The second least significant bit (0x2) of the stream ID distinguishes
        //# between bidirectional streams (with the bit set to 0) and
        //# unidirectional streams (with the bit set to 1).
        if Into::<u64>::into(self.0) & 0x02 == 0 {
            StreamType::Bidirectional
        } else {
            StreamType::Unidirectional
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varint::MAX_VARINT_VALUE;

    #[test]
    fn initial_stream_ids() {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for initiator in &[endpoint::Type::Client, endpoint::Type::Server] {
                let id = StreamId::initial(*initiator, *stream_type);
                assert_eq!(*stream_type, id.stream_type());
                assert_eq!(*initiator, id.initiator());
            }
        }
    }

    #[test]
    fn stream_id_overflow() {
        // Check that the highest possible Stream ID works
        let max_stream_id_varint = VarInt::new((1 << 62) - 1).unwrap();
        let _max_stream_id = StreamId::from_varint(max_stream_id_varint);

        let max_increaseable_stream_id_varint = max_stream_id_varint - 4;
        let max_inreasable_stream_id = StreamId::from_varint(max_increaseable_stream_id_varint);
        assert!(max_inreasable_stream_id.next_of_type().is_some());

        // Check all the variants where the base ID is still valid but the
        // increment is no longer.
        for increment in 1..5 {
            let id_varint = max_increaseable_stream_id_varint + increment;
            let stream_id = StreamId::from_varint(id_varint);
            assert!(stream_id.next_of_type().is_none());
        }
    }

    #[test]
    fn nth_stream_id() {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for initiator in &[endpoint::Type::Client, endpoint::Type::Server] {
                // The first StreamId is the initial one
                let first = StreamId::nth(*initiator, *stream_type, 0).unwrap();
                assert_eq!(StreamId::initial(*initiator, *stream_type), first);

                for n in 1..10 {
                    let nth = StreamId::nth(*initiator, *stream_type, n).unwrap();
                    assert_eq!(VarInt::from_u32(n as u32 * 4), nth.0 - first.0);
                }
            }
        }
    }

    #[test]
    fn invalid_nth_stream_id() {
        for stream_type in &[StreamType::Bidirectional, StreamType::Unidirectional] {
            for initiator in &[endpoint::Type::Client, endpoint::Type::Server] {
                assert_eq!(
                    None,
                    StreamId::nth(
                        *initiator,
                        *stream_type,
                        Into::<u64>::into(MAX_VARINT_VALUE / 2)
                    )
                );
            }
        }
    }
}
