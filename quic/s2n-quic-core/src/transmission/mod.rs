// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame::ack_elicitation::{AckElicitable, AckElicitation},
    packet::number,
};

pub mod constraint;
pub mod mode;

pub use constraint::Constraint;
pub use mode::Mode;

#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub ack_elicitation: AckElicitation,
    pub is_congestion_controlled: bool,
    pub bytes_sent: usize,
    pub packet_number: number::PacketNumber,
}

impl Outcome {
    pub fn new(packet_number: number::PacketNumber) -> Outcome {
        Outcome {
            ack_elicitation: AckElicitation::NonEliciting,
            is_congestion_controlled: false,
            bytes_sent: 0,
            packet_number,
        }
    }
}

impl AckElicitable for Outcome {
    fn ack_elicitation(&self) -> AckElicitation {
        self.ack_elicitation
    }
}
