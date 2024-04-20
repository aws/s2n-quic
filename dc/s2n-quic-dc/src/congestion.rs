// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    event, random,
    recovery::{
        bbr::BbrCongestionController, congestion_controller::Publisher, CongestionController,
        RttEstimator,
    },
    time::{timer, Timestamp},
};

pub type PacketInfo = <BbrCongestionController as CongestionController>::PacketInfo;

#[derive(Clone, Debug)]
pub struct Controller {
    controller: BbrCongestionController,
}

impl Controller {
    #[inline]
    pub fn new(mtu: u16) -> Self {
        let mut controller = BbrCongestionController::new(mtu);
        let publisher = &mut NoopPublisher;
        controller.on_mtu_update(mtu, publisher);
        Self { controller }
    }

    #[inline]
    pub fn on_packet_sent(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: u16,
        has_more_app_data: bool,
        rtt_estimator: &RttEstimator,
    ) -> PacketInfo {
        let sent_bytes = sent_bytes as usize;
        let app_limited = Some(!has_more_app_data);
        let publisher = &mut NoopPublisher;
        self.controller
            .on_packet_sent(time_sent, sent_bytes, app_limited, rtt_estimator, publisher)
    }

    #[inline]
    pub fn on_packet_ack(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acked: usize,
        newest_acked_packet_info: PacketInfo,
        rtt_estimator: &RttEstimator,
        random_generator: &mut dyn random::Generator,
        ack_receive_time: Timestamp,
    ) {
        let publisher = &mut NoopPublisher;
        self.controller.on_ack(
            newest_acked_time_sent,
            bytes_acked,
            newest_acked_packet_info,
            rtt_estimator,
            random_generator,
            ack_receive_time,
            publisher,
        )
    }

    #[inline]
    pub fn on_explicit_congestion(&mut self, ce_count: u64, now: Timestamp) {
        let publisher = &mut NoopPublisher;
        self.controller
            .on_explicit_congestion(ce_count, now, publisher);
    }

    #[inline]
    pub fn on_packet_lost(
        &mut self,
        bytes_lost: u32,
        packet_info: PacketInfo,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
    ) {
        // TODO where do these come from?
        let persistent_congestion = false;
        let new_loss_burst = false;

        let publisher = &mut NoopPublisher;
        self.controller.on_packet_lost(
            bytes_lost,
            packet_info,
            persistent_congestion,
            new_loss_burst,
            random_generator,
            now,
            publisher,
        );
    }

    #[inline]
    pub fn is_congestion_limited(&self) -> bool {
        self.controller.is_congestion_limited()
    }

    #[inline]
    pub fn requires_fast_retransmission(&self) -> bool {
        self.controller.requires_fast_retransmission()
    }

    #[inline]
    pub fn congestion_window(&self) -> u32 {
        self.controller.congestion_window()
    }

    #[inline]
    pub fn bytes_in_flight(&self) -> u32 {
        self.controller.bytes_in_flight()
    }

    #[inline]
    pub fn send_quantum(&self) -> usize {
        self.controller.send_quantum().unwrap_or(usize::MAX)
    }

    #[inline]
    pub fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.controller.earliest_departure_time()
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        if let Some(time) = self.earliest_departure_time() {
            let mut timer = timer::Timer::default();
            timer.set(time);
            query.on_timer(&timer)?;
        }
        Ok(())
    }
}

struct NoopPublisher;

impl Publisher for NoopPublisher {
    #[inline]
    fn on_slow_start_exited(
        &mut self,
        _cause: event::builder::SlowStartExitCause,
        _congestion_window: u32,
    ) {
        // TODO
    }

    #[inline]
    fn on_delivery_rate_sampled(
        &mut self,
        _rate_sample: s2n_quic_core::recovery::bandwidth::RateSample,
    ) {
        // TODO
    }

    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        _pacing_rate: s2n_quic_core::recovery::bandwidth::Bandwidth,
        _burst_size: u32,
        _pacing_gain: num_rational::Ratio<u64>,
    ) {
        // TODO
    }

    #[inline]
    fn on_bbr_state_changed(&mut self, _state: event::builder::BbrState) {
        // TODO
    }
}
