// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    packet::number::PacketNumberSpace,
    path,
    path::MINIMUM_MAX_DATAGRAM_SIZE,
    random,
    recovery::{
        congestion_controller::PathPublisher, CongestionController, CubicCongestionController,
        RttEstimator,
    },
    time::{Clock, NoopClock, Timestamp},
};
use core::{fmt, ops::Range, time::Duration};
use insta::assert_debug_snapshot;
use plotters::prelude::*;
use std::{
    env,
    path::{Path, PathBuf},
};

const CHART_DIMENSIONS: (u32, u32) = (1024, 768);

fn type_name<T>() -> &'static str {
    core::any::type_name::<T>().split("::").last().unwrap()
}

// These simulations are too slow for Miri
#[test]
#[cfg_attr(miri, ignore)]
fn slow_start_unlimited_test() {
    let cc = CubicCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    slow_start_unlimited(cc, 12).finish();
}

#[test]
#[cfg_attr(miri, ignore)]
fn loss_at_3mb_test() {
    let cc = CubicCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    loss_at_3mb(cc, 135).finish();
}

#[test]
#[cfg_attr(miri, ignore)]
fn app_limited_1mb_test() {
    let cc = CubicCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    app_limited_1mb(cc, 120).finish();
}

#[test]
#[cfg_attr(miri, ignore)]
fn minimum_window_test() {
    let cc = CubicCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    minimum_window(cc, 10).finish();
}

#[test]
#[cfg_attr(miri, ignore)]
fn loss_at_3mb_and_2_75mb_test() {
    let cc = CubicCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    loss_at_3mb_and_2_75mb(cc, 120).finish();
}

#[derive(Debug)]
struct Simulation {
    name: &'static str,
    description: &'static str,
    cc: &'static str,
    rounds: Vec<Round>,
}

struct Round {
    number: usize,
    cwnd: u32,
}

impl fmt::Debug for Round {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Print out packet count rather than bytes to normalize any differences between
        // platforms.
        //
        // Floating point numbers are not guaranteed to be deterministic between CPU
        // architectures, compilers, or libc implementations.
        let packets = self.cwnd / MINIMUM_MAX_DATAGRAM_SIZE as u32;

        write!(f, "{:>3}: pkts: {}", self.number, packets)
    }
}

impl Simulation {
    fn finish(&self) {
        if let Ok(dir) = env::var("RECOVERY_SIM_DIR") {
            let mut path = PathBuf::new();
            path.push(dir);
            path.push(self.filename());
            path.set_extension("svg");
            self.plot(&path);
        } else {
            self.assert_snapshot();
        }
    }

    fn plot<T: AsRef<Path> + ?Sized>(&self, path: &T) {
        let root_area = SVGBackend::new(path, CHART_DIMENSIONS).into_drawing_area();
        root_area.fill(&WHITE).expect("Could not fill chart");
        root_area
            .titled(&self.name(), ("sans-serif", 40))
            .expect("Could not add title");

        let mut ctx = ChartBuilder::on(&root_area)
            .set_label_area_size(LabelAreaPosition::Left, 120)
            .set_label_area_size(LabelAreaPosition::Bottom, 60)
            .margin(20)
            .margin_top(40)
            .caption(self.description, ("sans-serif", 20))
            .build_cartesian_2d(self.x_spec(), self.y_spec())
            .expect("Could not build chart");

        ctx.configure_mesh()
            .x_desc("Transmission Round")
            .label_style(("sans-serif", 20))
            .y_desc("Congestion window size (bytes)")
            .draw()
            .expect("Could not configure mesh");

        ctx.draw_series(LineSeries::new(
            self.rounds.iter().map(|x| (x.number as i32, x.cwnd as i32)),
            GREEN,
        ))
        .expect("Could not draw series");
    }

    fn x_spec(&self) -> Range<i32> {
        0..(self.rounds.len() as i32 + 1)
    }

    fn y_spec(&self) -> Range<i32> {
        let mut max = self.rounds.iter().map(|r| r.cwnd as i32).max().unwrap_or(0);

        // Add a 5% buffer
        max = (max as f32 * 1.05) as i32;

        0..max
    }

    fn assert_snapshot(&self) {
        assert_debug_snapshot!(self.filename(), self);
    }

    fn name(&self) -> String {
        let mut name = String::new();
        name.push_str(self.name);
        name.push_str(" - ");
        name.push_str(self.cc.split("::").last().unwrap());
        name
    }

    fn filename(&self) -> String {
        self.name().replace('.', "_").split_whitespace().collect()
    }
}

/// Simulates a network with no congestion experienced
fn slow_start_unlimited<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    Simulation {
        name: "Slow Start Unlimited",
        description: "Full congestion window utilization with no congestion experienced",
        cc: type_name::<CC>(),
        rounds: simulate_constant_rtt(&mut congestion_controller, &[], None, num_rounds),
    }
}

/// Simulates a network that experienced loss at a 3MB congestion window
fn loss_at_3mb<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    Simulation {
        name: "Loss at 3MB",
        description: "Full congestion window utilization with loss encountered at ~3MB",
        cc: type_name::<CC>(),
        rounds: simulate_constant_rtt(&mut congestion_controller, &[3_000_000], None, num_rounds),
    }
}

/// Simulates a network that experiences loss at a 750KB congestion window with the application
/// sending at most 1MB of data per round.
fn app_limited_1mb<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    const APP_LIMIT_BYTES: usize = 1_000_000;

    Simulation {
        name: "App Limited 1MB",
        description: "App limited to 1MB per round with loss encountered at ~750KB",
        cc: type_name::<CC>(),
        rounds: simulate_constant_rtt(
            &mut congestion_controller,
            &[750_000],
            Some(APP_LIMIT_BYTES),
            num_rounds,
        ),
    }
}

/// Simulates a network starting from the minimum window size with no further congestion
fn minimum_window<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    let time_zero = NoopClock.get_time();
    let rtt_estimator = RttEstimator::default();
    let random = &mut random::testing::Generator::default();
    let mut publisher = event::testing::Publisher::no_snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    let packet_info = congestion_controller.on_packet_sent(
        time_zero,
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        None,
        &rtt_estimator,
        &mut publisher,
    );
    // Experience persistent congestion to drop to the minimum window
    congestion_controller.on_packet_lost(
        MINIMUM_MAX_DATAGRAM_SIZE as u32,
        packet_info,
        true,
        false,
        random,
        time_zero,
        &mut publisher,
    );
    let packet_info = congestion_controller.on_packet_sent(
        time_zero,
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        None,
        &rtt_estimator,
        &mut publisher,
    );
    // Lose a packet to exit slow start
    congestion_controller.on_packet_lost(
        MINIMUM_MAX_DATAGRAM_SIZE as u32,
        packet_info,
        false,
        false,
        random,
        time_zero,
        &mut publisher,
    );

    Simulation {
        name: "Minimum Window",
        description: "Full congestion window utilization after starting from the minimum window",
        cc: type_name::<CC>(),
        rounds: simulate_constant_rtt(&mut congestion_controller, &[], None, num_rounds),
    }
}

/// Simulates a network that experienced loss at a 3MB congestion window and then at a 2.75MB window
/// With Cubic this will exercise the fast convergence algorithm
fn loss_at_3mb_and_2_75mb<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    Simulation {
        name: "Loss at 3MB and 2.75MB",
        description: "Loss encountered at ~3MB and ~2.75MB",
        cc: type_name::<CC>(),
        rounds: simulate_constant_rtt(
            &mut congestion_controller,
            &[3_000_000, 2_750_000],
            None,
            num_rounds,
        ),
    }
}

/// Simulate the given number of rounds with drops occurring at the given congestion window sizes
/// and limited to the given app limit
fn simulate_constant_rtt<CC: CongestionController>(
    congestion_controller: &mut CC,
    drops: &[u32],
    app_limit: Option<usize>,
    num_rounds: usize,
) -> Vec<Round> {
    let time_zero = NoopClock.get_time();
    let mut rtt_estimator = RttEstimator::default();
    let random = &mut random::testing::Generator::default();
    let mut publisher = event::testing::Publisher::no_snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    // Update the rtt with 200 ms
    rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(200),
        time_zero,
        true,
        PacketNumberSpace::ApplicationData,
    );
    let mut round_start = NoopClock.get_time() + Duration::from_millis(1);
    let mut rounds = Vec::with_capacity(num_rounds);
    let mut drop_index = 0;

    for round in 0..num_rounds {
        rounds.push(Round {
            number: round,
            cwnd: congestion_controller.congestion_window(),
        });

        round_start += Duration::from_millis(200);

        if drop_index < drops.len()
            && congestion_controller.congestion_window() >= drops[drop_index]
        {
            // Drop a packet
            let packet_info = congestion_controller.on_packet_sent(
                round_start,
                MINIMUM_MAX_DATAGRAM_SIZE as usize,
                None,
                &rtt_estimator,
                &mut publisher,
            );
            congestion_controller.on_packet_lost(
                MINIMUM_MAX_DATAGRAM_SIZE as u32,
                packet_info,
                false,
                false,
                random,
                round_start,
                &mut publisher,
            );
            drop_index += 1;
        } else {
            let send_bytes = (congestion_controller.congestion_window() as usize)
                .min(app_limit.unwrap_or(usize::MAX));

            // Send and ack the full congestion window
            send_and_ack(
                congestion_controller,
                &rtt_estimator,
                round_start,
                send_bytes,
            );
        }
    }

    rounds
}

/// Send and acknowledge the given amount of bytes using the given congestion controller
fn send_and_ack<CC: CongestionController>(
    congestion_controller: &mut CC,
    rtt_estimator: &RttEstimator,
    timestamp: Timestamp,
    bytes: usize,
) {
    let random = &mut random::testing::Generator::default();
    let mut tx_remaining = bytes;
    let mut rx_remaining = 0;
    let mut now = timestamp;
    let ack_receive_time = now + rtt_estimator.min_rtt();
    // Allow acks to start being received after this time, to simulate
    // acks arriving while sending is paused by the pacer.
    let earliest_ack_receive_time = ack_receive_time - Duration::from_millis(50);
    let sending_full_cwnd = bytes as u32 == congestion_controller.congestion_window();
    let mut publisher = event::testing::Publisher::no_snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    let mut packet_info = None;

    while tx_remaining > 0 || rx_remaining > 0 {
        while tx_remaining > 0 {
            if let Some(edt) = congestion_controller.earliest_departure_time() {
                if !edt.has_elapsed(now) {
                    // We are blocked by the pacer, stop sending and fast forward to the earliest departure time
                    now = edt;
                    break;
                }
            }

            let bytes_sent = tx_remaining.min(MINIMUM_MAX_DATAGRAM_SIZE as usize);
            let app_limited = tx_remaining - bytes_sent == 0 && !sending_full_cwnd;

            packet_info = Some(congestion_controller.on_packet_sent(
                now,
                bytes_sent,
                Some(app_limited),
                rtt_estimator,
                &mut publisher,
            ));
            tx_remaining -= bytes_sent;
            rx_remaining += bytes_sent;
        }

        if tx_remaining == 0 {
            // Nothing left to send, so fast forward to when we receive acks for everything
            now = ack_receive_time;
        }

        while now >= earliest_ack_receive_time && rx_remaining > 0 {
            let bytes_acked = rx_remaining.min(MINIMUM_MAX_DATAGRAM_SIZE as usize);

            congestion_controller.on_ack(
                now,
                bytes_acked,
                packet_info.unwrap(),
                rtt_estimator,
                random,
                now,
                &mut publisher,
            );
            rx_remaining -= bytes_acked;
        }
    }
}
