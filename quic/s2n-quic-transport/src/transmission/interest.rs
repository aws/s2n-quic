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

            // transmit lost data when we're either not limited or we want to do a fast
            // retransmission to try to recover
            (Interest::LostData, Constraint::None) => true,
            (Interest::LostData, Constraint::RetransmissionOnly) => true,
            (Interest::LostData, _) => false,

            // new data may only be transmitted when we're not limited
            (Interest::NewData, Constraint::None) => true,
            (Interest::NewData, _) => false,

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
