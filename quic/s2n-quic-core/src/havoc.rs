// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame::{self, new_connection_id::STATELESS_RESET_TOKEN_LEN},
    stream::StreamType,
    varint,
};
use core::ops::Range;
pub use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};

pub trait Random {
    fn fill(&mut self, bytes: &mut [u8]);

    fn gen_range(&mut self, range: Range<u64>) -> u64;

    #[inline]
    fn shuffle(&mut self, bytes: &mut [u8]) {
        if bytes.is_empty() {
            return;
        }

        let len = bytes.len() as u64;

        let count = self.gen_range(0..len);
        for _ in 0..count {
            let from = self.gen_range(0..len);
            let to = self.gen_range(0..len);
            bytes.swap(from as usize, to as usize);
        }
    }

    #[inline]
    fn gen_slice<'a>(&mut self, bytes: &'a mut [u8]) -> &'a mut [u8] {
        if bytes.is_empty() {
            return bytes;
        }

        let len = bytes.len() as u64;
        let len = self.gen_range(0..len);
        let bytes = &mut bytes[..len as usize];
        self.fill(bytes);
        bytes
    }

    #[inline]
    fn gen_bool(&mut self) -> bool {
        self.gen_u8() & 0b1 == 0b1
    }

    #[inline]
    fn gen_u8(&mut self) -> u8 {
        let mut o = [0];
        self.fill(&mut o);
        o[0]
    }

    #[inline]
    fn gen_u32(&mut self) -> u32 {
        let mut o = [0; 4];
        self.fill(&mut o);
        u32::from_le_bytes(o)
    }

    #[inline]
    fn gen_u64(&mut self) -> u64 {
        let mut o = [0; 8];
        self.fill(&mut o);
        u64::from_le_bytes(o)
    }

    #[inline]
    fn gen_varint(&mut self) -> varint::VarInt {
        use varint::VarInt;
        let max = VarInt::MAX.as_u64();
        let v = self.gen_range(0..(max + 1));
        unsafe { VarInt::new_unchecked(v as _) }
    }
}

pub trait Strategy: Sized {
    /// Applies the havoc strategy to the supplied buffer
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer);

    /// Applies the havoc strategy to the supplied buffer slice and returns the new buffer length
    #[inline]
    fn havoc_slice<R: Random>(&mut self, rand: &mut R, buffer: &mut [u8]) -> usize {
        let mut buffer = EncoderBuffer::new(buffer);
        self.havoc(rand, &mut buffer);
        buffer.len()
    }

    /// Applies the havoc strategy to the given `u16` value and returns the new `u16` result
    #[inline]
    fn havoc_u16<R: Random>(&mut self, rand: &mut R, input: u16) -> u16 {
        let buffer = &mut [0; 2];
        let mut buffer = EncoderBuffer::new(buffer);
        buffer.encode(&input);

        self.havoc(rand, &mut buffer);

        buffer
            .as_mut_slice()
            .try_into()
            .map(u16::from_be_bytes)
            .unwrap_or(input)
    }

    /// Alternate between two strategies with the supplied `period`
    fn alternate<B: Strategy>(self, b: B, period: Range<usize>) -> Alternate<Self, B> {
        Alternate::new(self, b, period)
    }

    /// Apply the strategy `count` times
    fn repeat(self, count: Range<usize>) -> Repeat<Self> {
        Repeat::new(self, count)
    }

    /// Randomly apply the strategy
    fn randomly(self) -> Randomly<Self> {
        Randomly { strategy: self }
    }

    /// Toggle the strategy on and off for the supplied `period`
    fn toggle(self, period: Range<usize>) -> Toggle<Self> {
        Toggle::new(self, period)
    }

    /// Applies two strategies in order
    fn and_then<B: Strategy>(self, b: B) -> AndThen<Self, B> {
        AndThen { a: self, b }
    }

    /// Repeatedly applies the strategy as long as the buffer has capacity
    fn while_has_capacity(self) -> WhileHasCapacity<Self> {
        WhileHasCapacity { strategy: self }
    }

    /// Applies the strategy and holds the result `count` times
    #[cfg(feature = "alloc")]
    fn hold(self, count: Range<usize>) -> Hold<Self> {
        Hold::new(self, count)
    }
}

impl<T: Strategy> Strategy for Option<T> {
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        if let Some(strategy) = self.as_mut() {
            strategy.havoc(rand, buffer);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Alternate<A: Strategy, B: Strategy> {
    a: A,
    b: B,
    min: u64,
    max: u64,
    is_a: bool,
    remaining: u64,
    init: bool,
}

impl<A: Strategy, B: Strategy> Alternate<A, B> {
    pub fn new(a: A, b: B, range: Range<usize>) -> Self {
        debug_assert_ne!(range.start, 0);
        debug_assert!(range.start <= range.end);
        Self {
            a,
            b,
            min: range.start as _,
            max: range.end as _,
            is_a: false,
            remaining: 0,
            init: false,
        }
    }
}

impl<A: Strategy, B: Strategy> Strategy for Alternate<A, B> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        // initialize the strategy to a random arm
        if !self.init {
            self.init = true;
            self.is_a = rand.gen_bool();
        }

        loop {
            if let Some(remaining) = self.remaining.checked_sub(1) {
                self.remaining = remaining;

                if self.is_a {
                    self.a.havoc(rand, buffer);
                } else {
                    self.b.havoc(rand, buffer);
                }

                break;
            }

            self.remaining = rand.gen_range(self.min..self.max);
            self.is_a = !self.is_a;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AndThen<A: Strategy, B: Strategy> {
    pub a: A,
    pub b: B,
}

impl<A: Strategy, B: Strategy> Strategy for AndThen<A, B> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        self.a.havoc(rand, buffer);
        self.b.havoc(rand, buffer);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Repeat<S: Strategy> {
    strategy: S,
    min: u64,
    max: u64,
}

impl<S: Strategy> Repeat<S> {
    pub fn new(strategy: S, range: Range<usize>) -> Self {
        Self {
            strategy,
            min: range.start as _,
            max: range.end as _,
        }
    }
}

impl<S: Strategy> Strategy for Repeat<S> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        let count = rand.gen_range(self.min..self.max);
        for _ in 0..count {
            self.strategy.havoc(rand, buffer);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WhileHasCapacity<S: Strategy> {
    strategy: S,
}

impl<S: Strategy> WhileHasCapacity<S> {
    pub fn new(strategy: S) -> Self {
        Self { strategy }
    }
}

impl<S: Strategy> Strategy for WhileHasCapacity<S> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        // limit the number of iterations in case the strategy isn't filling the buffer
        for _ in 0..25 {
            if !buffer.remaining_capacity() == 0 {
                break;
            }

            self.strategy.havoc(rand, buffer);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Toggle<S: Strategy> {
    alt: Alternate<Disabled, S>,
}

impl<S: Strategy> Toggle<S> {
    pub fn new(strategy: S, range: Range<usize>) -> Self {
        Self {
            alt: Alternate::new(Disabled, strategy, range),
        }
    }
}

impl<S: Strategy> Strategy for Toggle<S> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        self.alt.havoc(rand, buffer);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Randomly<S: Strategy> {
    pub strategy: S,
}

impl<S: Strategy> Strategy for Randomly<S> {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        if rand.gen_bool() {
            self.strategy.havoc(rand, buffer);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reset;

impl Strategy for Reset {
    #[inline]
    fn havoc<R: Random>(&mut self, _rand: &mut R, buffer: &mut EncoderBuffer) {
        buffer.set_position(0);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Zero;

impl Strategy for Zero {
    #[inline]
    fn havoc<R: Random>(&mut self, _rand: &mut R, buffer: &mut EncoderBuffer) {
        for byte in buffer.as_mut_slice() {
            *byte = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Shuffle;

impl Strategy for Shuffle {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        if !buffer.is_empty() {
            rand.shuffle(buffer.as_mut_slice());
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Swap;

impl Strategy for Swap {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        let len = buffer.len() as u64;
        if len > 0 {
            let from = rand.gen_range(0..len) as usize;
            let to = rand.gen_range(0..len) as usize;
            buffer.as_mut_slice().swap(from, to);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Truncate;

impl Strategy for Truncate {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        let len = buffer.capacity() as u64;
        if len > 0 {
            let new_len = rand.gen_range(0..len) as usize;
            buffer.set_position(new_len);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mutate;

impl Strategy for Mutate {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        let len = buffer.len() as u64;
        if len > 0 {
            let index = rand.gen_range(0..len) as usize;
            let value = rand.gen_u8();
            buffer.as_mut_slice()[index] = value;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Disabled;

impl Strategy for Disabled {
    #[inline]
    fn havoc<R: Random>(&mut self, _rand: &mut R, _buffer: &mut EncoderBuffer) {}
}

#[cfg(feature = "alloc")]
pub use hold::Hold;

#[cfg(feature = "alloc")]
mod hold {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Clone, Debug, Default)]
    pub struct Hold<S: Strategy> {
        strategy: S,
        min: u64,
        max: u64,
        value: Vec<u8>,
        remaining: u64,
    }

    impl<S: Strategy> Hold<S> {
        pub fn new(strategy: S, range: Range<usize>) -> Self {
            debug_assert!(range.start <= range.end);
            Self {
                strategy,
                min: range.start as _,
                max: range.end as _,
                value: Vec::new(),
                remaining: 0,
            }
        }
    }

    impl<S: Strategy> Strategy for Hold<S> {
        #[inline]
        fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
            if self.remaining == 0 {
                self.strategy.havoc(rand, buffer);
                // store the value after the strategy has been applied
                self.value.clear();
                self.value.extend_from_slice(buffer.as_mut_slice());
                self.remaining = rand.gen_range(self.min..self.max);
            } else {
                if !self.value.is_empty() {
                    // restore the value from the first application of the strategy
                    let len = buffer.capacity().min(self.value.len());
                    buffer.set_position(0);
                    buffer.write_slice(&self.value.as_mut_slice()[..len]);
                }
                self.remaining -= 1;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VarInt {
    min: u64,
    max: u64,
}

impl VarInt {
    pub fn new(range: Range<varint::VarInt>) -> Self {
        let min = range.start.as_u64();
        let max = range.end.as_u64();
        Self { min, max }
    }
}

impl Strategy for VarInt {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        let value = rand.gen_range(self.min..self.max);
        let value: varint::VarInt = value.try_into().unwrap();
        if value.encoding_size() <= buffer.remaining_capacity() {
            buffer.encode(&value);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Frame;

impl Strategy for Frame {
    #[inline]
    fn havoc<R: Random>(&mut self, rand: &mut R, buffer: &mut EncoderBuffer) {
        type GenFrame<R> = for<'a> fn(
            rand: &'a mut R,
            payload: &'a mut [u8],
            remaining_capacity: usize,
        ) -> frame::Frame<'a, AckRanges<'a, R>, &'a mut [u8]>;

        let frames: &[GenFrame<R>] = &[
            |rand, _data, cap| {
                frame::Padding {
                    length: rand.gen_range(1..cap as _) as _,
                }
                .into()
            },
            |_rand, _data, _cap| frame::Ping.into(),
            // TODO ACK
            |rand, _data, _cap| {
                frame::ResetStream {
                    stream_id: rand.gen_varint(),
                    application_error_code: rand.gen_varint(),
                    final_size: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::StopSending {
                    stream_id: rand.gen_varint(),
                    application_error_code: rand.gen_varint(),
                }
                .into()
            },
            |rand, data, cap| {
                let data = rand.gen_slice(&mut data[..cap]);
                frame::Crypto {
                    offset: rand.gen_varint(),
                    data,
                }
                .into()
            },
            |rand, data, cap| {
                let token = rand.gen_slice(&mut data[..cap]);
                frame::NewToken { token }.into()
            },
            |rand, data, cap| {
                let data = rand.gen_slice(&mut data[..cap]);
                frame::Stream {
                    stream_id: rand.gen_varint(),
                    offset: rand.gen_varint(),
                    data,
                    is_last_frame: rand.gen_bool(),
                    is_fin: rand.gen_bool(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::MaxData {
                    maximum_data: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::MaxStreamData {
                    stream_id: rand.gen_varint(),
                    maximum_stream_data: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::MaxStreams {
                    stream_type: if rand.gen_bool() {
                        StreamType::Unidirectional
                    } else {
                        StreamType::Bidirectional
                    },
                    maximum_streams: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::DataBlocked {
                    data_limit: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::StreamDataBlocked {
                    stream_id: rand.gen_varint(),
                    stream_data_limit: rand.gen_varint(),
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::StreamsBlocked {
                    stream_type: if rand.gen_bool() {
                        StreamType::Unidirectional
                    } else {
                        StreamType::Bidirectional
                    },
                    stream_limit: rand.gen_varint(),
                }
                .into()
            },
            |rand, data, cap| {
                let (stateless_reset_token, data) = data.split_at_mut(STATELESS_RESET_TOKEN_LEN);

                rand.fill(stateless_reset_token);
                let stateless_reset_token = (&*stateless_reset_token).try_into().unwrap();

                // connection ID lengths are encoded with a u8
                let cap = cap.min(u8::MAX as usize);
                let connection_id = rand.gen_slice(&mut data[..cap]);

                frame::NewConnectionId {
                    sequence_number: rand.gen_varint(),
                    retire_prior_to: rand.gen_varint(),
                    connection_id,
                    stateless_reset_token,
                }
                .into()
            },
            |rand, _data, _cap| {
                frame::RetireConnectionId {
                    sequence_number: rand.gen_varint(),
                }
                .into()
            },
            |rand, data, _cap| {
                let data = &mut data[..frame::path_challenge::DATA_LEN];
                rand.fill(data);
                let data = (&*data).try_into().unwrap();
                frame::PathChallenge { data }.into()
            },
            |rand, data, _cap| {
                let data = &mut data[..frame::path_challenge::DATA_LEN];
                rand.fill(data);
                let data = (&*data).try_into().unwrap();
                frame::PathResponse { data }.into()
            },
            |rand, data, cap| {
                frame::ConnectionClose {
                    error_code: rand.gen_varint(),
                    frame_type: if rand.gen_bool() {
                        Some(rand.gen_varint())
                    } else {
                        None
                    },
                    reason: if rand.gen_bool() {
                        let reason = rand.gen_slice(&mut data[..cap]);
                        Some(reason)
                    } else {
                        None
                    },
                }
                .into()
            },
            |_rand, _data, _cap| frame::HandshakeDone.into(),
            |rand, data, cap| {
                let data = rand.gen_slice(&mut data[..cap]);
                frame::Datagram {
                    is_last_frame: rand.gen_bool(),
                    data,
                }
                .into()
            },
        ];

        let index = rand.gen_range(0..frames.len() as u64) as usize;
        let mut payload = [0u8; 16_000];
        let frame = frames[index](rand, &mut payload, buffer.remaining_capacity());

        if frame.encoding_size() <= buffer.remaining_capacity() {
            buffer.encode(&frame);
        }
    }
}

// TODO implement this without allocating a bunch for the entries
struct AckRanges<'a, R> {
    #[allow(dead_code)]
    rand: &'a mut R,
}

impl<'a, R> frame::ack::AckRanges for AckRanges<'a, R> {
    type Iter = frame::ack::AckRangesIter<'a>;

    fn ack_ranges(&self) -> Self::Iter {
        todo!()
    }

    fn largest_acknowledged(&self) -> crate::varint::VarInt {
        todo!()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    pub struct RandomSlice<'a>(core::iter::Cycle<core::slice::Iter<'a, u8>>);

    impl<'a> RandomSlice<'a> {
        pub fn new(slice: &'a [u8]) -> Self {
            Self(slice.iter().cycle())
        }
    }

    impl Random for RandomSlice<'_> {
        #[inline]
        fn fill(&mut self, bytes: &mut [u8]) {
            for byte in bytes.iter_mut() {
                *byte = *self.0.next().unwrap_or(&0);
            }
        }

        #[inline]
        fn gen_range(&mut self, range: Range<u64>) -> u64 {
            let start = range.start.min(range.end);
            let end = range.start.max(range.end);

            // this method is biased and should not be used outside of this test

            let variance = end - start;

            // check to see if they're the same number
            if variance == 0 {
                return start;
            }

            let value = self.gen_u64();
            start + value % variance
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::RandomSlice, *};
    use bolero::check;

    macro_rules! test {
        ($name:ident, $strategy:expr) => {
            #[test]
            #[cfg_attr(miri, ignore)] // no need to test with miri as there isn't any unsafe
            fn $name() {
                check!().for_each(|bytes| {
                    let mut rand = RandomSlice::new(bytes);
                    let mut buffer = [0u8; 256];
                    let buffer = &mut buffer[..rand.gen_u8() as usize];
                    let mut buffer = EncoderBuffer::new(buffer);

                    let mut strategy = $strategy;

                    for _ in 0..rand.gen_range(1..10) {
                        strategy.havoc(&mut rand, &mut buffer);
                    }
                });
            }
        };
    }

    test!(disabled_test, Disabled);
    test!(truncate_test, Truncate);
    test!(reset_test, Reset);
    test!(zero_test, Zero);
    test!(swap_test, Swap);
    test!(shuffle_test, Shuffle);
    test!(mutate_test, Mutate);
    test!(alternate_test, Disabled.alternate(Disabled, 1..5));
    test!(and_then_test, Disabled.and_then(Disabled));
    test!(repeat_test, Disabled.repeat(0..5));
    test!(varint_test, VarInt::new(0u8.into()..42u8.into()));
    test!(frame_test, Frame);
    test!(while_has_capacity_test, Frame.while_has_capacity());
    test!(toggle_test, Disabled.toggle(1..5));
    test!(randomly_test, Disabled.randomly());
    test!(hold_test, Disabled.hold(0..5));
}
