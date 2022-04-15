// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::network::{Buffers, Network, Packet};
use core::time::Duration;
use s2n_quic_core::path::MaxMtu;
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicU16, AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Clone, Default)]
pub struct Model(Arc<State>);

impl Model {
    fn jitter(&self) -> Duration {
        Duration::from_micros(self.0.jitter.load(Ordering::SeqCst))
    }

    /// The amount of time between sending packets
    ///
    /// Setting this value to 0 will transmit all allowed packets at the exact same time.
    pub fn set_jitter(&self, value: Duration) -> &Self {
        self.0
            .jitter
            .store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    fn network_jitter(&self) -> Duration {
        Duration::from_micros(self.0.network_jitter.load(Ordering::SeqCst))
    }

    /// The amount of jitter in the network itself
    ///
    /// Setting this value to `>0` will cause packets to be reordered.
    pub fn set_network_jitter(&self, value: Duration) -> &Self {
        self.0
            .network_jitter
            .store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    fn delay(&self) -> Duration {
        Duration::from_micros(self.0.delay.load(Ordering::SeqCst))
    }

    /// The amount of time a packet is delayed before the receiver is able to read it
    pub fn set_delay(&self, value: Duration) -> &Self {
        self.0.delay.store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    fn transmit_rate(&self) -> u64 {
        self.0.transmit_rate.load(Ordering::SeqCst)
    }

    /// The number of packets that can be transmitted in a single round.
    ///
    /// By default, all packet buffers will be cleared on every round.
    pub fn set_transmit_rate(&self, value: u64) -> &Self {
        self.0.transmit_rate.store(value, Ordering::SeqCst);
        self
    }

    fn retransmit_rate(&self) -> u64 {
        self.0.retransmit_rate.load(Ordering::SeqCst)
    }

    /// The odds a packet will be retransmitted.
    ///
    /// Each packet will make an independent decision with odds of 1 in N.
    pub fn set_retransmit_rate(&self, value: u64) -> &Self {
        self.0.retransmit_rate.store(value, Ordering::SeqCst);
        self
    }

    fn corrupt_rate(&self) -> u64 {
        self.0.corrupt_rate.load(Ordering::SeqCst)
    }

    /// The odds a packet will be corrupted.
    ///
    /// Each packet will make an independent decision with odds of 1 in N.
    pub fn set_corrupt_rate(&self, value: f64) -> &Self {
        let value = rate_to_u64(value);
        self.0.corrupt_rate.store(value, Ordering::SeqCst);
        self
    }

    fn drop_rate(&self) -> u64 {
        self.0.drop_rate.load(Ordering::SeqCst)
    }

    /// The odds a packet will be dropped.
    ///
    /// Each packet will make an independent decision with odds of 1 in N.
    pub fn set_drop_rate(&self, value: f64) -> &Self {
        let value = rate_to_u64(value);
        self.0.drop_rate.store(value, Ordering::SeqCst);
        self
    }

    fn mtu(&self) -> u16 {
        self.0.mtu.load(Ordering::SeqCst)
    }

    /// The maximum payload size for the network
    ///
    /// NOTE: this is the UDP payload and doesn't include Ethernet/IP/UDP headers
    pub fn set_mtu(&self, value: u16) -> &Self {
        self.0.mtu.store(value, Ordering::SeqCst);
        self
    }

    /// The number of inflight packets
    fn inflight(&self) -> u64 {
        self.0.current_inflight.load(Ordering::SeqCst)
    }

    fn max_inflight(&self) -> u64 {
        self.0.max_inflight.load(Ordering::SeqCst)
    }

    /// Sets the maximum number of packets that can be inflight for the network
    ///
    /// Any packets that exceed this amount will be dropped
    pub fn set_max_inflight(&self, value: u64) -> &Self {
        self.0.max_inflight.store(value, Ordering::SeqCst);
        self
    }
}

fn rate_to_u64(rate: f64) -> u64 {
    let value = rate.max(0.0).min(1.0);
    let value = value * u64::MAX as f64;
    value.round() as u64
}

struct State {
    delay: AtomicU64,
    jitter: AtomicU64,
    network_jitter: AtomicU64,
    transmit_rate: AtomicU64,
    retransmit_rate: AtomicU64,
    corrupt_rate: AtomicU64,
    drop_rate: AtomicU64,
    mtu: AtomicU16,
    max_inflight: AtomicU64,
    current_inflight: AtomicU64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            delay: AtomicU64::new(Duration::from_millis(50).as_micros() as _),
            jitter: AtomicU64::new(0),
            network_jitter: AtomicU64::new(0),
            transmit_rate: AtomicU64::new(u64::MAX),
            retransmit_rate: AtomicU64::new(0),
            corrupt_rate: AtomicU64::new(0),
            drop_rate: AtomicU64::new(0),
            mtu: AtomicU16::new(MaxMtu::default().into()),
            max_inflight: AtomicU64::new(u64::MAX),
            current_inflight: AtomicU64::new(0),
        }
    }
}

impl Network for Model {
    fn execute(&mut self, buffers: &Buffers) -> usize {
        let jitter = self.jitter();
        let network_jitter = self.network_jitter();
        let transmit_rate = self.transmit_rate();
        let retransmit_rate = self.retransmit_rate();
        let corrupt_rate = self.corrupt_rate();
        let drop_rate = self.drop_rate();
        let mtu = self.mtu() as usize;

        let now = super::time::now();
        let mut transmit_time = now + self.delay();
        let transmit_time = &mut transmit_time;

        let mut transmit = |packet: Cow<Packet>| {
            // drop the packet if it's over the current MTU
            if packet.payload.len() > mtu {
                return 0;
            }

            // drop the packet if enabled
            if drop_rate > 0 && super::rand::gen::<u64>() < drop_rate {
                return 0;
            }

            let mut packet = packet.into_owned();

            if corrupt_rate > 0 && super::rand::gen::<u64>() < corrupt_rate {
                // randomly truncate the payload
                let num_bytes = super::rand::gen_range(0..packet.payload.len());
                if num_bytes > 0 {
                    packet.payload.truncate(num_bytes);
                }

                // randomly swap bytes in the payload
                let num_bytes = super::rand::gen_range(0..packet.payload.len());
                if num_bytes > 0 {
                    super::rand::swap_count(&mut packet.payload, num_bytes);
                }

                // randomly rewrite bytes in the payload
                let num_bytes = super::rand::gen_range(0..packet.payload.len());
                if num_bytes > 0 {
                    for _ in 0..num_bytes {
                        let index = super::rand::gen_range(0..packet.payload.len());
                        packet.payload[index] = super::rand::gen();
                    }
                }
            }

            if !jitter.is_zero() {
                // add a delay for the next packet to be transmitted
                *transmit_time += gen_jitter(jitter);
            }

            // copy the transmit time for this packet
            let mut transmit_time = *transmit_time;

            if !network_jitter.is_zero() {
                let jitter = gen_jitter(network_jitter);

                // randomly add or subtract the network jitter
                if super::rand::gen() {
                    transmit_time += jitter;
                } else {
                    transmit_time = transmit_time.checked_sub(jitter).unwrap_or(now);
                }
            }

            // reverse the addresses so the dst/src are correct for the receiver
            packet.switch();

            let model = self.clone();
            model.0.current_inflight.fetch_add(1, Ordering::SeqCst);
            let buffers = buffers.clone();

            // spawn a task that will push the packet onto the receiver queue at the transit time
            super::spawn(async move {
                if now != transmit_time {
                    super::time::delay_until(transmit_time).await;
                }

                buffers.rx(*packet.path.local_address, |queue| {
                    model.0.current_inflight.fetch_sub(1, Ordering::SeqCst);
                    queue.receive(packet);
                });
            });

            1
        };

        let mut transmission_count = 0;
        buffers.pending_transmissions(|packet| {
            // drop packets that exceed the maximum number of inflight packets for the network
            if self.inflight() >= self.max_inflight() {
                return Ok(());
            }

            // retransmit the packet until the rate fails or we retransmit 5
            let mut count = 0;
            while retransmit_rate > 0
                && count < 5
                && super::rand::gen_range(0..retransmit_rate) == 0
            {
                transmission_count += transmit(Cow::Borrowed(&packet));
                count += 1;
            }

            transmission_count += transmit(Cow::Owned(packet));

            // continue transmitting as long as we are under the rate
            if transmission_count < transmit_rate {
                Ok(())
            } else {
                Err(())
            }
        });

        transmission_count as usize
    }
}

fn gen_jitter(max_jitter: Duration) -> Duration {
    let micros = super::rand::gen_range(0..max_jitter.as_micros() as u64);
    let micros = micros as f64;
    // even though we're generated micros, we round to the nearest millisecond
    // so packets can be grouped together
    let millis = micros / 1000.0;
    let millis = f64::round(millis) as u64;
    Duration::from_millis(millis)
}
