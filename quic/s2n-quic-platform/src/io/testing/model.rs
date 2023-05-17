// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::network::{Buffers, Network, Packet};
use core::time::Duration;
use s2n_quic_core::{havoc, path::MaxMtu};
use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicU16, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Default)]
pub struct TxRecorder {
    packets: Arc<Mutex<Vec<Packet>>>,
}

impl TxRecorder {
    pub fn get_packets(&self) -> Arc<Mutex<Vec<Packet>>> {
        self.packets.clone()
    }
}

impl Network for TxRecorder {
    fn execute(&mut self, buffers: &Buffers) -> usize {
        let mut packets = self.packets.lock().unwrap();
        buffers.pending_transmission(|packet| {
            packets.push(packet.clone());
        });
        0
    }
}

#[derive(Clone, Default)]
pub struct Model(Arc<State>);

impl Model {
    pub fn jitter(&self) -> Duration {
        Duration::from_micros(self.0.jitter.load(Ordering::SeqCst))
    }

    /// The amount of time between sending packets
    ///
    /// Setting this value to `0` will transmit all allowed packets at the exact same time.
    pub fn set_jitter(&self, value: Duration) -> &Self {
        self.0
            .jitter
            .store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    pub fn network_jitter(&self) -> Duration {
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

    pub fn delay(&self) -> Duration {
        Duration::from_micros(self.0.delay.load(Ordering::SeqCst))
    }

    /// The amount of time a packet is delayed before the receiver is able to read it
    pub fn set_delay(&self, value: Duration) -> &Self {
        self.0.delay.store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    pub fn transmit_rate(&self) -> u64 {
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
    /// Each packet will make an independent decision with odds of `0.0..1.0`, with `0.0` having no
    /// chance and `1.0` occurring with each packet.
    pub fn set_retransmit_rate(&self, value: f64) -> &Self {
        let value = rate_to_u64(value);
        self.0.retransmit_rate.store(value, Ordering::SeqCst);
        self
    }

    fn corrupt_rate(&self) -> u64 {
        self.0.corrupt_rate.load(Ordering::SeqCst)
    }

    /// The odds a packet will be corrupted.
    ///
    /// Each packet will make an independent decision with odds of `0.0..1.0`, with `0.0` having no
    /// chance and `1.0` occurring with each packet.
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
    /// Each packet will make an independent decision with odds of `0.0..1.0`, with `0.0` having no
    /// chance and `1.0` occurring with each packet.
    pub fn set_drop_rate(&self, value: f64) -> &Self {
        let value = rate_to_u64(value);
        self.0.drop_rate.store(value, Ordering::SeqCst);
        self
    }

    pub fn max_udp_payload(&self) -> u16 {
        self.0.max_udp_payload.load(Ordering::SeqCst)
    }

    /// The maximum payload size for the network
    pub fn set_max_udp_payload(&self, value: u16) -> &Self {
        self.0.max_udp_payload.store(value, Ordering::SeqCst);
        self
    }

    /// The number of inflight packets
    fn inflight(&self) -> u64 {
        self.0.current_inflight.load(Ordering::SeqCst)
    }

    pub fn max_inflight(&self) -> u64 {
        self.0.max_inflight.load(Ordering::SeqCst)
    }

    /// Sets the maximum number of packets that can be inflight for the network
    ///
    /// Any packets that exceed this amount will be dropped
    pub fn set_max_inflight(&self, value: u64) -> &Self {
        self.0.max_inflight.store(value, Ordering::SeqCst);
        self
    }

    pub fn inflight_delay(&self) -> Duration {
        Duration::from_micros(self.0.inflight_delay.load(Ordering::SeqCst))
    }

    /// Sets the delay for each packet above the inflight_delay
    pub fn set_inflight_delay(&self, value: Duration) -> &Self {
        self.0
            .inflight_delay
            .store(value.as_micros() as _, Ordering::SeqCst);
        self
    }

    pub fn inflight_delay_threshold(&self) -> u64 {
        self.0.inflight_delay_threshold.load(Ordering::SeqCst)
    }

    /// Sets the delay for each packet above the inflight_delay_threshold
    pub fn set_inflight_delay_threshold(&self, value: u64) -> &Self {
        self.0
            .inflight_delay_threshold
            .store(value, Ordering::SeqCst);
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
    max_udp_payload: AtomicU16,
    max_inflight: AtomicU64,
    inflight_delay: AtomicU64,
    inflight_delay_threshold: AtomicU64,
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
            max_udp_payload: AtomicU16::new(MaxMtu::default().into()),
            max_inflight: AtomicU64::new(u64::MAX),
            inflight_delay: AtomicU64::new(0),
            inflight_delay_threshold: AtomicU64::new(u64::MAX),
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
        let max_udp_payload = self.max_udp_payload() as usize;
        let inflight_delay = self.inflight_delay();
        let inflight_delay_threshold = self.inflight_delay_threshold();

        let now = super::time::now();
        let mut transmit_time = now + self.delay();
        let transmit_time = &mut transmit_time;

        #[inline]
        fn gen_rate(rate: u64) -> bool {
            // ensure the rate isn't 0 before actually generating a random number
            rate > 0 && super::rand::gen::<u64>() < rate
        }

        let mut transmit = |packet: Cow<Packet>| {
            // drop the packet if it's over the current MTU
            if packet.payload.len() > max_udp_payload {
                return 0;
            }

            // drop packets that exceed the maximum number of inflight packets for the network
            if self.inflight() >= self.max_inflight() {
                return 0;
            }

            // drop the packet if enabled
            if gen_rate(drop_rate) {
                return 0;
            }

            let mut packet = packet.into_owned();

            if !packet.payload.is_empty() && gen_rate(corrupt_rate) {
                use havoc::Strategy as _;

                let new_len = havoc::Truncate
                    .randomly()
                    .and_then(havoc::Swap.repeat(0..packet.payload.len()).randomly())
                    .and_then(havoc::Mutate.repeat(0..packet.payload.len()).randomly())
                    .havoc_slice(&mut super::rand::Havoc, &mut packet.payload);

                // if the len was changed, then update it
                if new_len != packet.payload.len() {
                    packet.payload.truncate(new_len);
                }
            }

            if !jitter.is_zero() {
                // add a delay for the next packet to be transmitted
                *transmit_time += gen_jitter(jitter);
            }

            // copy the transmit time for this packet
            let mut transmit_time = *transmit_time;

            if !network_jitter.is_zero() {
                transmit_time += gen_jitter(network_jitter);
            }

            let model = self.clone();
            let current_inflight = model.0.current_inflight.fetch_add(1, Ordering::SeqCst);

            // scale the inflight delay by the number above the delay threshold
            if let Some(mul) = current_inflight.checked_sub(inflight_delay_threshold) {
                transmit_time += inflight_delay * mul as u32;
            }

            // reverse the addresses so the dst/src are correct for the receiver
            packet.switch();

            let buffers = buffers.clone();

            // spawn a task that will push the packet onto the receiver queue at the transit time
            super::spawn(async move {
                // if the packet isn't scheduled to transmit immediately, wait until the computed
                // time
                if now != transmit_time {
                    super::time::delay_until(transmit_time).await;
                }

                buffers.record(&packet);
                buffers.rx(*packet.path.local_address, |queue| {
                    model.0.current_inflight.fetch_sub(1, Ordering::SeqCst);
                    queue.receive(packet);
                });
            });

            1
        };

        let mut transmission_count = 0;
        buffers.drain_pending_transmissions(|packet| {
            buffers.record(&packet);

            // retransmit the packet until the rate fails or we retransmit 5
            //
            // We limit retransmissions to 5 just so we don't endlessly iterate when the
            // `retransmit_rate` is high. This _should_ be high enough where we're getting
            // retransmission coverage without needlessly saturating the network.
            let mut count = 0;
            while count < 5 && gen_rate(retransmit_rate) {
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
