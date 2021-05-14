// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::{BitOr, BitOrAssign};

/// Describes if a frame is probing
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Probe {
    NonProbing,
    Probing,
}

impl Probe {
    /// Returns true if the `Probe` is set to `Probing`
    pub fn is_probing(self) -> bool {
        matches!(self, Self::Probing)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.1
//# A packet containing only probing frames is a "probing packet", and a
//# packet containing any other frame is a "non-probing packet".
impl BitOr<Probe> for Probe {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Probing, Self::Probing) => Self::Probing,
            (_, _) => Self::NonProbing,
        }
    }
}

impl BitOrAssign<Probe> for Probe {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}


/// Trait to retrieve if a frame is probing
pub trait Probable {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::NonProbing
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.1
//# PATH_CHALLENGE, PATH_RESPONSE, NEW_CONNECTION_ID, and PADDING frames
//# are "probing frames", and all other frames are "non-probing frames".
impl<AckRanges> Probable for crate::frame::Ack<AckRanges>{}
impl Probable for crate::frame::ConnectionClose<'_>{}
impl<Data> Probable for crate::frame::Crypto<Data> {}
impl Probable for crate::frame::DataBlocked {}
impl Probable for crate::frame::HandshakeDone {}
impl Probable for crate::frame::MaxData {}
impl Probable for crate::frame::MaxStreamData {}
impl Probable for crate::frame::MaxStreams {}
impl Probable for crate::frame::NewConnectionId<'_>
{
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl Probable for crate::frame::NewToken<'_> {}
impl Probable for crate::frame::Padding
{
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl Probable for crate::frame::PathChallenge<'_>
{
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl Probable for crate::frame::PathResponse<'_>
{
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl Probable for crate::frame::Ping {}
impl Probable for crate::frame::ResetStream {}
impl Probable for crate::frame::RetireConnectionId {}
impl Probable for crate::frame::StopSending {}
impl<Data> Probable for crate::frame::Stream<Data> {}
impl Probable for crate::frame::StreamDataBlocked {}
impl Probable for crate::frame::StreamsBlocked {}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn probing_packet_test() {
        let mut probe = Probe::Probing;
        probe |= Probe::Probing;

        assert_eq!(true, probe.is_probing())
    }

    #[test]
    fn probing_and_non_probing_packet_test() {
        let mut probe = Probe::Probing;
        probe |= Probe::NonProbing;

        assert_eq!(false, probe.is_probing())
    }
}
