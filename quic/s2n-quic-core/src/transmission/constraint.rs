#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    AmplificationLimited,
    CongestionLimited,
    FastRetransmission,
    None,
}

impl Constraint {
    pub fn is_amplification_limited(self) -> bool {
        matches!(self, Self::AmplificationLimited)
    }

    pub fn is_congestion_limited(self) -> bool {
        matches!(self, Self::CongestionLimited)
    }

    pub fn is_fast_retransmission(self) -> bool {
        matches!(self, Self::FastRetransmission)
    }

    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub fn can_transmit(self) -> bool {
        self.is_none()
    }

    pub fn can_retransmit(self) -> bool {
        self.is_none() || self.is_fast_retransmission()
    }
}
