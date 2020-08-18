// TODO: Remove when used
#![allow(dead_code)]

use crate::{recovery::K_GRANULARITY, timer::VirtualTimer};
use alloc::collections::BTreeMap;
use core::cmp::max;
use s2n_quic_core::{packet::number::PacketNumberSpace, recovery::RTTEstimator, time::Timestamp};

pub struct LossDetectionTimer {
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The number of times a PTO has been sent without receiving an ack.
    pto_count: u32,
    loss_time: BTreeMap<PacketNumberSpace, Timestamp>,
    time_of_last_ack_eliciting_packet: BTreeMap<PacketNumberSpace, Timestamp>,
    timer: VirtualTimer,
}

impl LossDetectionTimer {
    /// Constructs a new `LossDetectionTimer`
    pub fn new() -> Self {
        Self {
            pto_count: 0,
            loss_time: BTreeMap::default(),
            time_of_last_ack_eliciting_packet: BTreeMap::default(),
            timer: VirtualTimer::default(),
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.5
    //# Gets the earliest loss time and associated packet space.
    fn get_loss_time_and_space(&self) -> Option<(PacketNumberSpace, Timestamp)> {
        let mut loss_time_and_space: Option<(PacketNumberSpace, Timestamp)> = None;

        for (&pn_space, &loss_time) in self.loss_time.iter() {
            if let Some(time_and_space) = loss_time_and_space {
                if loss_time < time_and_space.1 {
                    loss_time_and_space = Some((pn_space, loss_time));
                }
            } else {
                loss_time_and_space = Some((pn_space, loss_time));
            }
        }

        loss_time_and_space
    }

    fn get_pto_time_and_space(
        &self,
        rtt_estimator: &RTTEstimator,
        no_inflight_packets: bool,
    ) -> Option<(PacketNumberSpace, Timestamp)> {
        let duration = (rtt_estimator.smoothed_rtt()
            + max(4 * rtt_estimator.rttvar(), K_GRANULARITY))
            * (2_i32.pow(self.pto_count)) as u32;
        // Arm PTO from now when there are no inflight packets.
        if no_inflight_packets {
            // assert(!PeerCompletedAddressValidation())
            // if (has handshake keys):
            // return (now() + duration), Handshake
            // else:
            // return (now() + duration), Initial
        }

        let mut pto_time_and_space: Option<(PacketNumberSpace, Timestamp)> = None;

        for &pn_space in [
            PacketNumberSpace::Initial,
            PacketNumberSpace::Handshake,
            PacketNumberSpace::ApplicationData,
        ]
        .iter()
        {
            //if (no in-flight packets in space):
            //         continue;
            if pn_space.is_application_data() {
                // // Skip ApplicationData until handshake complete.
                // if (handshake is not complete):
                // return pto_timeout, pto_space
                // // Include max_ack_delay and backoff for ApplicationData.
                // duration += max_ack_delay * (2 ^ pto_count)
            }

            if let Some(time_of_last_ack_eliciting_packet) =
                self.time_of_last_ack_eliciting_packet.get(&pn_space)
            {
                let t = *time_of_last_ack_eliciting_packet + duration;

                if let Some(time_and_space) = pto_time_and_space {
                    if t < time_and_space.1 {
                        pto_time_and_space = Some((pn_space, t));
                    }
                } else {
                    pto_time_and_space = Some((pn_space, t));
                }
            }
        }

        pto_time_and_space
    }

    pub fn set_loss_detection_timer(
        &mut self,
        rtt_estimator: &RTTEstimator,
        no_inflight_packets: bool,
        peer_completed_address_validation: bool,
        at_anti_amplification_limit: bool,
    ) {
        if let Some(earliest_loss_time) = self.get_loss_time_and_space() {
            // Time threshold loss detection.
            self.timer.set(earliest_loss_time.1);
            return;
        }

        if at_anti_amplification_limit {
            // The server's timer is not set if nothing can be sent.
            self.timer.cancel();
            return;
        }

        if no_inflight_packets && peer_completed_address_validation {
            // There is nothing to detect lost, so no timer is set.
            // However, the client needs to arm the timer if the
            // server might be blocked by the anti-amplification limit.
            self.timer.cancel();
            return;
        }

        // Determine which PN space to arm PTO for.
        if let Some(pto_time_and_space) =
            self.get_pto_time_and_space(rtt_estimator, no_inflight_packets)
        {
            self.timer.set(pto_time_and_space.1);
        }
    }

    pub fn on_loss_detection_timeout(&self) {
        if let Some(_earliest_loss_time) = self.get_loss_time_and_space() {
            // Time threshold loss Detection
        }
    }
}

#[cfg(test)]
mod test {}
