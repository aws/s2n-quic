// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::frame::ack_elicitation::{AckElicitable, AckElicitation};

pub mod constraint;

pub use constraint::Constraint;

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    pub ack_elicitation: AckElicitation,
    pub is_congestion_controlled: bool,
    pub bytes_sent: usize,
}

impl AckElicitable for Outcome {
    fn ack_elicitation(&self) -> AckElicitation {
        self.ack_elicitation
    }
}
