use core::ops::{BitOr, BitOrAssign};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#2
//# Ack-eliciting Packets:  Packets that contain ack-eliciting frames
//#    elicit an ACK from the receiver within the maximum ack delay and
//#    are called ack-eliciting packets.

/// Describes if a frame or packet requires an ACK from the peer
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AckElicitation {
    NonEliciting,
    Eliciting,
}

impl Default for AckElicitation {
    fn default() -> Self {
        Self::NonEliciting
    }
}

impl AckElicitation {
    /// Returns true if the `AckElicitation` is set to `Eliciting`
    pub fn is_ack_eliciting(self) -> bool {
        match self {
            Self::Eliciting => true,
            _ => false,
        }
    }
}

impl BitOr<AckElicitation> for AckElicitation {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Eliciting, _) => Self::Eliciting,
            (_, Self::Eliciting) => Self::Eliciting,
            (_, _) => Self::NonEliciting,
        }
    }
}

impl BitOrAssign<AckElicitation> for AckElicitation {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

/// Trait to retrieve the AckElicitation for a given value
pub trait AckElicitable {
    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        AckElicitation::Eliciting
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#2
//# Ack-eliciting Frames:  All frames other than ACK, PADDING, and
//#    CONNECTION_CLOSE are considered ack-eliciting.

impl<AckRanges> AckElicitable for crate::frame::Ack<AckRanges> {
    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        AckElicitation::NonEliciting
    }
}
impl AckElicitable for crate::frame::ConnectionClose<'_> {
    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        AckElicitation::NonEliciting
    }
}
impl<Data> AckElicitable for crate::frame::Crypto<Data> {}
impl AckElicitable for crate::frame::DataBlocked {}
impl AckElicitable for crate::frame::HandshakeDone {}
impl AckElicitable for crate::frame::MaxData {}
impl AckElicitable for crate::frame::MaxStreamData {}
impl AckElicitable for crate::frame::MaxStreams {}
impl AckElicitable for crate::frame::NewConnectionID<'_> {}
impl AckElicitable for crate::frame::NewToken<'_> {}
impl AckElicitable for crate::frame::Padding {
    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        AckElicitation::NonEliciting
    }
}
impl AckElicitable for crate::frame::PathChallenge<'_> {}
impl AckElicitable for crate::frame::PathResponse<'_> {}
impl AckElicitable for crate::frame::Ping {}
impl AckElicitable for crate::frame::ResetStream {}
impl AckElicitable for crate::frame::RetireConnectionID {}
impl AckElicitable for crate::frame::StopSending {}
impl<Data> AckElicitable for crate::frame::Stream<Data> {}
impl AckElicitable for crate::frame::StreamDataBlocked {}
impl AckElicitable for crate::frame::StreamsBlocked {}
