// TODO: Remove when used
#![allow(dead_code)]

use crate::{
    recovery::{RecoveryManager, K_GRANULARITY},
    timer::VirtualTimer,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{packet::number::PacketNumberSpace, path::Path, time::Timestamp};

pub struct LossDetectionTimer {
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The number of times a PTO has been sent without receiving an ack.
    pto_count: u32,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# Multi-modal timer used for loss detection.
    timer: VirtualTimer,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The maximum amount of time by which the receiver intends to delay acknowledgments for packets
    //# in the ApplicationData packet number space. The actual ack_delay in a received ACK frame may
    //# be larger due to late timers, reordering, or lost ACK frames.
    max_ack_delay: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LossDetectionInfo {
    pub pn_space: PacketNumberSpace,
    pub loss_time: Option<Timestamp>,
    pub time_of_last_ack_eliciting_packet: Option<Timestamp>,
}

impl LossDetectionInfo {
    /// Constructs a new `LossDetectionInfo`
    pub fn new(pn_space: PacketNumberSpace) -> Self {
        Self {
            pn_space,
            loss_time: None,
            time_of_last_ack_eliciting_packet: None,
        }
    }
}

impl LossDetectionTimer {
    /// Constructs a new `LossDetectionTimer`
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            pto_count: 0,
            timer: VirtualTimer::default(),
            max_ack_delay,
        }
    }

    /// Resets the `pto_count` to zero
    pub fn reset_pto_count(&mut self) {
        self.pto_count = 0;
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.8
    /// Gets the `LossDetectionInfo` with the earliest loss time.
    fn get_loss_time_and_space(
        loss_detection_info: impl Iterator<Item = LossDetectionInfo>,
    ) -> Option<LossDetectionInfo> {
        loss_detection_info.min_by_key(|l| l.loss_time)
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.8
    fn get_pto_time_and_space(
        &self,
        path: &Path,
        loss_detection_info: impl Iterator<Item = LossDetectionInfo> + Clone,
        has_handshake_keys: bool,
        is_handshake_complete: bool,
        now: Timestamp,
    ) -> Option<(PacketNumberSpace, Timestamp)> {
        let backoff = 2_u32.pow(self.pto_count);
        let duration = (path.rtt_estimator.smoothed_rtt()
            + max(4 * path.rtt_estimator.rttvar(), K_GRANULARITY))
            * backoff;
        // Arm PTO from now when there are no inflight packets.
        if loss_detection_info.clone().next().is_none() {
            assert!(!path.is_validated());
            if has_handshake_keys {
                return Some((PacketNumberSpace::Handshake, now + duration));
            } else {
                return Some((PacketNumberSpace::Initial, now + duration));
            }
        }

        let mut pto_time_and_space: Option<(PacketNumberSpace, Timestamp)> = None;

        for loss_detection_info in loss_detection_info {
            if loss_detection_info.pn_space.is_application_data() && !is_handshake_complete {
                // Skip ApplicationData until handshake complete.
                continue;
            }

            let mut timeout = loss_detection_info
                .time_of_last_ack_eliciting_packet
                .expect("ack eliciting packets must have been sent")
                + duration;

            // Include max_ack_delay and backoff for ApplicationData.
            if loss_detection_info.pn_space.is_application_data() {
                timeout += self.max_ack_delay * backoff;
            }

            if pto_time_and_space.map_or(true, |(_, t)| timeout < t) {
                pto_time_and_space = Some((loss_detection_info.pn_space, timeout));
            }
        }

        pto_time_and_space
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.8
    //# QUIC loss detection uses a single timer for all timeout loss detection.
    pub fn set_loss_detection_timer(
        &mut self,
        path: &Path,
        has_handshake_keys: bool,
        is_handshake_complete: bool,
        now: Timestamp,
        loss_detection_info: impl Iterator<Item = LossDetectionInfo> + Clone,
    ) {
        if let Some(earliest_loss_time) =
            LossDetectionTimer::get_loss_time_and_space(loss_detection_info.clone())
                .map(|l| l.loss_time)
                .flatten()
        {
            // Time threshold loss detection.
            self.timer.set(earliest_loss_time);
            return;
        }

        if path.at_amplification_limit() {
            // The server's timer is not set if nothing can be sent.
            self.timer.cancel();
            return;
        }

        if loss_detection_info.clone().next().is_none() && path.is_validated() {
            // There is nothing to detect lost, so no timer is set.
            // However, the client needs to arm the timer if the
            // server might be blocked by the anti-amplification limit.
            self.timer.cancel();
            return;
        }

        // Determine which PN space to arm PTO for.
        if let Some((_, pto_time)) = self.get_pto_time_and_space(
            path,
            loss_detection_info,
            has_handshake_keys,
            is_handshake_complete,
            now,
        ) {
            self.timer.set(pto_time);
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.9
    //# When the loss detection timer expires, the timer's mode determines the action to be performed.
    pub fn on_loss_detection_timeout(
        &mut self,
        path: &Path,
        loss_detection_info: impl Iterator<Item = LossDetectionInfo> + Clone,
        recovery_manager: &mut RecoveryManager,
        has_handshake_keys: bool,
        is_handshake_complete: bool,
        now: Timestamp,
    ) {
        if let Some(earliest_loss_time) =
            LossDetectionTimer::get_loss_time_and_space(loss_detection_info.clone())
                .map(|l| l.loss_time)
                .flatten()
        {
            // Time threshold loss Detection
            let lost_packets = recovery_manager.detect_and_remove_lost_packets(
                path.rtt_estimator.latest_rtt(),
                path.rtt_estimator.smoothed_rtt(),
                now,
                &mut Some(earliest_loss_time),
            );
            assert!(!lost_packets.is_empty());
            // TODO: congestion_controller.on_packets_lost(lost_packets)
            self.set_loss_detection_timer(
                path,
                has_handshake_keys,
                is_handshake_complete,
                now,
                loss_detection_info,
            );
            return;
        }

        // if congestion_controller.bytes_in_flight() > 0 {
        // PTO. Send new data if available, else retransmit old data.
        // If neither is available, send a single PING frame.
        // _, pn_space = loss_detection_timer.get_pto_time_and_space();
        // send_one_or_two_ack_eliciting_packets(pn_space)
        // else {
        // TODO: implement client
        // }

        self.pto_count += 1;
        self.set_loss_detection_timer(
            path,
            has_handshake_keys,
            is_handshake_complete,
            now,
            loss_detection_info,
        );
    }
}

#[cfg(test)]
mod test {
    use crate::recovery::{LossDetectionInfo, LossDetectionTimer};
    use core::{iter, time::Duration};
    use s2n_quic_core::{
        connection::ConnectionId, inet::SocketAddress, packet::number::PacketNumberSpace,
        path::Path, recovery::RTTEstimator,
    };

    #[test]
    fn get_loss_time_and_space() {
        let mut loss_detection_info_1 = LossDetectionInfo::new(PacketNumberSpace::Initial);
        let mut loss_detection_info_2 = LossDetectionInfo::new(PacketNumberSpace::Handshake);
        let mut loss_detection_info_3 = LossDetectionInfo::new(PacketNumberSpace::ApplicationData);

        let now = s2n_quic_platform::time::now();

        loss_detection_info_1.loss_time = Some(now);
        loss_detection_info_2.loss_time = Some(now + Duration::from_millis(1));
        loss_detection_info_3.loss_time = Some(now + Duration::from_millis(2));

        let mut loss_detection_info = Vec::new();
        loss_detection_info.push(loss_detection_info_3);
        loss_detection_info.push(loss_detection_info_2);
        loss_detection_info.push(loss_detection_info_1);

        assert_eq!(
            Some(loss_detection_info_1),
            LossDetectionTimer::get_loss_time_and_space(loss_detection_info.into_iter())
        );
    }
    #[test]
    fn reset_pto_count() {
        let mut loss_detection_timer = LossDetectionTimer::new(Duration::default());
        loss_detection_timer.pto_count = 100;
        loss_detection_timer.reset_pto_count();
        assert_eq!(0, loss_detection_timer.pto_count);
    }

    #[test]
    fn time_threshold_loss_detection() {
        let now = s2n_quic_platform::time::now();
        let mut loss_detection_timer = LossDetectionTimer::new(Duration::default());
        let mut loss_detection_info = LossDetectionInfo::new(PacketNumberSpace::Initial);
        let loss_time = now + Duration::from_millis(10);
        loss_detection_info.loss_time = Some(loss_time);

        let path = Path::new(
            ConnectionId::try_from_bytes(&[]).unwrap(),
            SocketAddress::default(),
            ConnectionId::try_from_bytes(&[]).unwrap(),
            RTTEstimator::new(Duration::default()),
        );

        assert!(!loss_detection_timer.timer.is_armed());

        loss_detection_timer.set_loss_detection_timer(
            &path,
            false,
            false,
            now,
            iter::once(loss_detection_info),
        );

        assert!(loss_detection_timer.timer.is_armed());
        assert_eq!(Some(&loss_time), loss_detection_timer.timer.iter().next());
    }
}
