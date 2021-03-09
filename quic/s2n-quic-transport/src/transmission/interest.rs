// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::transmission::Constraint;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interest {
    None,
    NewData,
    LostData,
    Forced,
}

#[test]
fn ordering_test() {
    assert!(Interest::None < Interest::NewData);
    assert!(Interest::NewData < Interest::LostData);
    assert!(Interest::LostData < Interest::Forced);
}

impl Default for Interest {
    fn default() -> Self {
        Self::None
    }
}

impl Interest {
    pub fn can_transmit(self, limit: Constraint) -> bool {
        match (self, limit) {
            // nothing can be transmitted when we're at amplification limits
            (_, Constraint::AmplificationLimited) => false,

            // a component wants to try to recover so ignore limits
            (Interest::Forced, _) => true,

            // transmit lost data when we're either not limited, probing, or we want to do a fast
            // retransmission to try to recover
            (Interest::LostData, _) => limit.can_retransmit(),

            // new data may only be transmitted when we're not limited or probing
            (Interest::NewData, _) => limit.can_transmit(),

            // nothing is interested in transmitting anything
            (Interest::None, _) => false,
        }
    }

    pub fn is_none(self) -> bool {
        matches!(self, Interest::None)
    }
}

impl core::ops::Add for Interest {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        self.max(rhs)
    }
}

impl core::ops::AddAssign for Interest {
    fn add_assign(&mut self, rhs: Self) {
        *self = (*self) + rhs;
    }
}

impl core::iter::Sum for Interest {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut interest = Self::default();

        for item in iter {
            interest += item;
        }

        interest
    }
}

pub trait Provider {
    fn transmission_interest(&self) -> Interest;
}

#[cfg(test)]
mod test {
    use crate::transmission::{
        Constraint,
        Constraint::*,
        Interest::{None, *},
    };

    #[test]
    fn can_transmit() {
        // Amplification Limited
        assert!(!None.can_transmit(AmplificationLimited));
        assert!(!NewData.can_transmit(AmplificationLimited));
        assert!(!LostData.can_transmit(AmplificationLimited));
        assert!(!Forced.can_transmit(AmplificationLimited));

        // Congestion Limited
        assert!(!None.can_transmit(CongestionLimited));
        assert!(!NewData.can_transmit(CongestionLimited));
        assert!(!LostData.can_transmit(CongestionLimited));
        assert!(Forced.can_transmit(CongestionLimited));

        // Retransmission Only
        assert!(!None.can_transmit(RetransmissionOnly));
        assert!(!NewData.can_transmit(RetransmissionOnly));
        assert!(LostData.can_transmit(RetransmissionOnly));
        assert!(Forced.can_transmit(RetransmissionOnly));

        // Probing
        assert!(!None.can_transmit(Constraint::Probing));
        assert!(NewData.can_transmit(Constraint::Probing));
        assert!(LostData.can_transmit(Constraint::Probing));
        assert!(Forced.can_transmit(Constraint::Probing));

        // No Constraint
        assert!(!None.can_transmit(Constraint::None));
        assert!(NewData.can_transmit(Constraint::None));
        assert!(LostData.can_transmit(Constraint::None));
        assert!(Forced.can_transmit(Constraint::None));
    }
}
