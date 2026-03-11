// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use lru::LruCache;
use rand::Rng as _;
use s2n_codec::encoder::scatter;
use s2n_quic_core::{
    event::api::Subject,
    havoc::{self, Strategy as _, *},
    packet::{
        self,
        interceptor::{DecoderBufferMut, Havoc},
    },
    path::RemoteAddress,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Intercept {
    #[structopt(long)]
    havoc_rx: bool,

    #[structopt(long)]
    havoc_tx: bool,

    #[structopt(long)]
    havoc_port: bool,
}

struct Random;

impl havoc::Random for Random {
    fn fill(&mut self, bytes: &mut [u8]) {
        rand::rng().fill_bytes(bytes);
    }

    fn gen_range(&mut self, range: std::ops::Range<u64>) -> u64 {
        let start = range.start.min(range.end);
        let end = range.start.max(range.end);

        // check to see if they're the same number
        if start == end {
            return start;
        }

        rand::random_range(start..end)
    }
}

type Strategy = Toggle<
    Alternate<
        AndThen<
            AndThen<AndThen<Disabled, Toggle<Shuffle>>, Toggle<Repeat<Swap>>>,
            Toggle<Repeat<Mutate>>,
        >,
        AndThen<Toggle<Reset>, WhileHasCapacity<Frame>>,
    >,
>;

type PortStrategy = Hold<Toggle<Mutate>>;

pub struct Interceptor {
    rx: bool,
    tx: bool,
    port: bool,
    strategies: LruCache<Option<u64>, Havoc<Strategy, Strategy, PortStrategy, Random>>,
}

impl Interceptor {
    fn strategy_for(
        &mut self,
        subject: &Subject,
    ) -> &mut Havoc<Strategy, Strategy, PortStrategy, Random> {
        let id = match subject {
            Subject::Connection { id, .. } => Some(*id),
            _ => None,
        };

        if !self.strategies.contains(&id) {
            let strategy = Self::strategy(1..100);
            let port_strategy = Self::port_strategy(1..5);

            let strategy = Havoc {
                rx: strategy.clone(),
                tx: strategy,
                port: port_strategy,
                random: Random,
            };

            self.strategies.push(id, strategy);
        }

        self.strategies.get_mut(&id).unwrap()
    }

    fn strategy(toggle: core::ops::Range<usize>) -> Strategy {
        Disabled
            .and_then(Shuffle.toggle(toggle.clone()))
            .and_then(Swap.repeat(1..16).toggle(toggle.clone()))
            .and_then(Mutate.repeat(1..16).toggle(toggle.clone()))
            .alternate(
                Reset
                    .toggle(toggle.clone())
                    .and_then(Frame.while_has_capacity()),
                toggle.clone(),
            )
            .toggle(toggle)
    }

    fn port_strategy(toggle: core::ops::Range<usize>) -> PortStrategy {
        // Hold the mutated port for a period to allow the
        // receiver to respond to the new port
        Mutate.toggle(toggle).hold(10..20)
    }
}

impl packet::interceptor::Interceptor for Interceptor {
    #[inline]
    fn intercept_rx_remote_address(&mut self, subject: &Subject, addr: &mut RemoteAddress) {
        if self.port {
            self.strategy_for(subject)
                .intercept_rx_remote_address(subject, addr)
        }
    }

    #[inline]
    fn intercept_rx_payload<'a>(
        &mut self,
        subject: &Subject,
        packet: &packet::interceptor::Packet,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        if !self.rx {
            return payload;
        }

        self.strategy_for(subject)
            .intercept_rx_payload(subject, packet, payload)
    }

    fn intercept_tx_payload(
        &mut self,
        subject: &Subject,
        packet: &packet::interceptor::Packet,
        payload: &mut scatter::Buffer,
    ) {
        if !self.tx {
            return;
        }

        self.strategy_for(subject)
            .intercept_tx_payload(subject, packet, payload)
    }
}

impl Intercept {
    pub fn interceptor(&self) -> Interceptor {
        Interceptor {
            rx: self.havoc_rx,
            tx: self.havoc_tx,
            port: self.havoc_port,
            strategies: LruCache::new(unsafe { core::num::NonZeroUsize::new_unchecked(10_000) }),
        }
    }
}
