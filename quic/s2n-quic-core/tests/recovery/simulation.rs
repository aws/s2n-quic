use core::{ops::Range, time::Duration};
use insta::assert_debug_snapshot;
use plotters::prelude::*;
use s2n_quic_core::{
    packet::number::PacketNumberSpace,
    path::MINIMUM_MTU,
    recovery::{CongestionController, CubicCongestionController, RTTEstimator},
    time::{Clock, NoopClock},
};
use std::path::Path;

const CHART_DIMENSIONS: (u32, u32) = (1024, 768);

fn main() {
    let cc = CubicCongestionController::new(MINIMUM_MTU);

    let simulation = slow_start_unlimited(cc, 12);
    simulation.plot("slow_start_unlimited.png");
    simulation.assert_snapshot();
}

#[derive(Debug)]
struct Simulation {
    name: &'static str,
    rounds: Vec<Round>,
}

#[derive(Debug)]
struct Round {
    number: usize,
    cwnd: u32,
}

impl Simulation {
    fn plot<'a, T: AsRef<Path> + ?Sized>(&self, path: &'a T) {
        let root_area = BitMapBackend::new(path, CHART_DIMENSIONS).into_drawing_area();
        root_area.fill(&WHITE).expect("Could not fill chart");

        let mut ctx = ChartBuilder::on(&root_area)
            .set_label_area_size(LabelAreaPosition::Left, 100)
            .set_label_area_size(LabelAreaPosition::Bottom, 60)
            .margin(20)
            .caption(self.name, ("sans-serif", 40))
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
            &GREEN,
        ))
        .expect("Could not draw series");
    }

    fn x_spec(&self) -> Range<i32> {
        0..(self.rounds.len() as i32 + 1)
    }

    fn y_spec(&self) -> Range<i32> {
        let max = self.rounds.iter().map(|r| r.cwnd as i32).max().unwrap_or(0);

        0..max + MINIMUM_MTU as i32
    }

    fn assert_snapshot(&self) {
        let snapshot_name = self.name.split_whitespace().collect::<String>();
        assert_debug_snapshot!(snapshot_name, self);
    }
}

/// Simulates a network with no congestion experienced
fn slow_start_unlimited<CC: CongestionController>(
    mut congestion_controller: CC,
    num_rounds: usize,
) -> Simulation {
    let mut bytes_in_flight = 0;
    let time_zero = NoopClock.get_time();
    let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(0));
    let mut rounds = Vec::with_capacity(num_rounds);
    let packet_size = 600;

    for round in 0..num_rounds {
        rounds.push(Round {
            number: round,
            cwnd: congestion_controller.congestion_window(),
        });

        let sent_time = time_zero + Duration::from_millis(100 * round as u64);

        // Send the full congestion window of bytes
        while bytes_in_flight <= congestion_controller.congestion_window() {
            congestion_controller.on_packet_sent(sent_time, packet_size);
            bytes_in_flight += packet_size as u32;
        }

        let ack_receive_time = sent_time + Duration::from_millis(50);

        // Ack the full congestion window of bytes
        while bytes_in_flight > 0 {
            congestion_controller.on_packet_ack(
                ack_receive_time,
                packet_size,
                &rtt_estimator,
                ack_receive_time,
            );
            bytes_in_flight -= packet_size as u32;
        }

        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            ack_receive_time - sent_time,
            ack_receive_time,
            true,
            PacketNumberSpace::ApplicationData,
        );
    }

    Simulation {
        name: "Slow Start Unlimited",
        rounds,
    }
}
