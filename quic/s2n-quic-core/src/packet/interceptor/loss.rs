// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{havoc, Interceptor};
use core::ops::Range;

#[derive(Debug)]
struct Direction {
    loss: Range<u64>,
    pass: Range<u64>,
    mode: Mode,
}

impl Default for Direction {
    fn default() -> Self {
        Self {
            loss: 0..0,
            pass: u64::MAX..u64::MAX,
            mode: Mode::Loss { remaining: 0 },
        }
    }
}

impl Direction {
    #[inline]
    fn should_pass<R: havoc::Random>(&mut self, random: &mut R) -> bool {
        let (remaining, mut is_pass) = match &mut self.mode {
            Mode::Pass { remaining } => (remaining, true),
            Mode::Loss { remaining } => (remaining, false),
        };

        // if we can decrement the remaining, then stay on the current mode
        if let Some(new_value) = remaining.checked_sub(1) {
            *remaining = new_value;
            return is_pass;
        }

        // try 3 times to generate the next value. It's good to be an odd number so we're at least
        // alternating if the ranges keep returning 0.
        for _ in 0..3 {
            // go to the next mode
            is_pass = !is_pass;

            let remaining = if is_pass {
                Self::gen_range(&self.pass, random)
            } else {
                Self::gen_range(&self.loss, random)
            };

            // if this round picked 0 then try again
            if remaining == 0 {
                continue;
            }

            if is_pass {
                self.mode = Mode::Pass { remaining };
            } else {
                self.mode = Mode::Loss { remaining };
            }
        }

        is_pass
    }

    #[inline]
    fn gen_range<R: havoc::Random>(range: &Range<u64>, random: &mut R) -> u64 {
        if range.start == range.end {
            return range.start;
        }

        random.gen_range(range.clone())
    }
}

#[derive(Debug)]
enum Mode {
    Loss { remaining: u64 },
    Pass { remaining: u64 },
}

#[derive(Debug, Default)]
pub struct Builder<R> {
    tx: Direction,
    rx: Direction,
    random: R,
}

impl<R> Builder<R>
where
    R: 'static + Send + havoc::Random,
{
    pub fn new(random: R) -> Self {
        Self {
            tx: Direction::default(),
            rx: Direction::default(),
            random,
        }
    }

    pub fn with_tx_pass(mut self, range: Range<u64>) -> Self {
        self.tx.pass = range;
        self
    }

    pub fn with_tx_loss(mut self, range: Range<u64>) -> Self {
        self.tx.loss = range;
        self
    }

    pub fn with_rx_pass(mut self, range: Range<u64>) -> Self {
        self.rx.pass = range;
        self
    }

    pub fn with_rx_loss(mut self, range: Range<u64>) -> Self {
        self.rx.loss = range;
        self
    }

    pub fn build(self) -> Loss<R> {
        Loss {
            tx: self.tx,
            rx: self.rx,
            random: self.random,
        }
    }
}

#[derive(Debug, Default)]
pub struct Loss<R>
where
    R: 'static + Send + havoc::Random,
{
    tx: Direction,
    rx: Direction,
    random: R,
}

impl<R> Loss<R>
where
    R: 'static + Send + havoc::Random,
{
    pub fn builder(random: R) -> Builder<R> {
        Builder::new(random)
    }
}

impl<R> Interceptor for Loss<R>
where
    R: 'static + Send + havoc::Random,
{
    #[inline]
    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &crate::event::api::Subject,
        _datagram: &super::Datagram,
        payload: s2n_codec::DecoderBufferMut<'a>,
    ) -> s2n_codec::DecoderBufferMut<'a> {
        if !self.rx.should_pass(&mut self.random) {
            return s2n_codec::DecoderBufferMut::new(&mut payload.into_less_safe_slice()[..0]);
        }

        payload
    }

    #[inline]
    fn intercept_tx_datagram(
        &mut self,
        _subject: &crate::event::api::Subject,
        _datagram: &super::Datagram,
        payload: &mut s2n_codec::EncoderBuffer,
    ) {
        if !self.tx.should_pass(&mut self.random) {
            payload.set_position(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{havoc::testing::RandomSlice, *};

    #[test]
    fn alternate_test() {
        static SLICE: &[u8] = &{
            let mut slice = [0u8; 256];
            let mut i = 0;
            while i < slice.len() {
                slice[i] = i as _;
                i += 1;
            }
            slice
        };

        let mut rand = RandomSlice::new(SLICE);

        let mut rx = Direction {
            loss: 0..10,
            pass: 1..10,
            ..Default::default()
        };

        let mut passed = 0;
        let mut dropped = 0;

        for _ in 0..256 {
            if rx.should_pass(&mut rand) {
                passed += 1;
            } else {
                dropped += 1;
            }
        }

        // these values will always be the same since the Random generator is deterministic
        assert_eq!(passed, 143);
        assert_eq!(dropped, 113);
    }
}
