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
pub trait PathProbing {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::NonProbing
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.1
//# PATH_CHALLENGE, PATH_RESPONSE, NEW_CONNECTION_ID, and PADDING frames
//# are "probing frames", and all other frames are "non-probing frames".
impl<AckRanges> PathProbing for crate::frame::Ack<AckRanges> {}
impl PathProbing for crate::frame::ConnectionClose<'_> {}
impl<Data> PathProbing for crate::frame::Crypto<Data> {}
impl PathProbing for crate::frame::DataBlocked {}
impl PathProbing for crate::frame::HandshakeDone {}
impl PathProbing for crate::frame::MaxData {}
impl PathProbing for crate::frame::MaxStreamData {}
impl PathProbing for crate::frame::MaxStreams {}
impl PathProbing for crate::frame::NewConnectionId<'_> {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl PathProbing for crate::frame::NewToken<'_> {}
impl PathProbing for crate::frame::Padding {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl PathProbing for crate::frame::PathChallenge<'_> {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl PathProbing for crate::frame::PathResponse<'_> {
    #[inline]
    fn probe(&self) -> Probe {
        Probe::Probing
    }
}
impl PathProbing for crate::frame::Ping {}
impl PathProbing for crate::frame::ResetStream {}
impl PathProbing for crate::frame::RetireConnectionId {}
impl PathProbing for crate::frame::StopSending {}
impl<Data> PathProbing for crate::frame::Stream<Data> {}
impl PathProbing for crate::frame::StreamDataBlocked {}
impl PathProbing for crate::frame::StreamsBlocked {}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.1
//= type=test
//# A packet containing only probing frames is a "probing packet", and a
//# packet containing any other frame is a "non-probing packet".
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
