#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    AmplificationLimited,
    CongestionLimited,
    RetransmissionOnly,
    None,
}

impl Constraint {
    pub fn is_amplification_limited(self) -> bool {
        matches!(self, Self::AmplificationLimited)
    }

    pub fn is_congestion_limited(self) -> bool {
        matches!(self, Self::CongestionLimited)
    }

    pub fn is_retransmission_only(self) -> bool {
        matches!(self, Self::RetransmissionOnly)
    }

    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub fn can_transmit(self) -> bool {
        self.is_none()
    }

    pub fn can_retransmit(self) -> bool {
        self.is_none() || self.is_retransmission_only()
    }
}
