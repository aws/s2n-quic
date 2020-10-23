#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Draining,
    Final,
}

impl Status {
    #[allow(dead_code)]
    pub fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }

    #[allow(dead_code)]
    pub fn is_draining(self) -> bool {
        matches!(self, Self::Draining)
    }

    #[allow(dead_code)]
    pub fn is_final(self) -> bool {
        matches!(self, Self::Final)
    }
}

impl core::ops::Add for Status {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        // all components need to be in their final state to finalize
        match (self, rhs) {
            // in idle the component doesn't report any finalization status
            (Self::Idle, rhs) => rhs,
            (lhs, Self::Idle) => lhs,

            // a draining component holds up finaliztion
            (Self::Draining, _) => Self::Draining,
            (_, Self::Draining) => Self::Draining,

            // only return final if both components are final
            (Self::Final, Self::Final) => Self::Final,
        }
    }
}

impl core::ops::AddAssign for Status {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl core::iter::Sum for Status {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut interest = Self::Idle;

        for item in iter {
            interest += item;
        }

        interest
    }
}

pub trait Provider {
    fn finalization_status(&self) -> Status;
}
