// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::{BitOr, BitOrAssign};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

/// Describes if a frame is probing
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
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

impl Default for Probe {
    /// A packet is Probing only if all frames within the packet are
    /// also Probing.
    ///
    /// This coupled with the Bit-Or logic makes `Probing` a good default:
    /// Probing | Probing = Probing
    /// Probing | NonProbing = NonProbing
    fn default() -> Self {
        Self::Probing
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.1
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
pub trait Probing {
    #[inline]
    fn path_validation(&self) -> Probe {
        Probe::NonProbing
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.1
//# PATH_CHALLENGE, PATH_RESPONSE, NEW_CONNECTION_ID, and PADDING frames
//# are "probing frames", and all other frames are "non-probing frames".
impl<AckRanges> Probing for crate::frame::Ack<AckRanges> {}
impl Probing for crate::frame::ConnectionClose<'_> {}
impl<Data> Probing for crate::frame::Crypto<Data> {}
impl<Data> Probing for crate::frame::Datagram<Data> {}
impl Probing for crate::frame::DataBlocked {}
impl Probing for crate::frame::DcStatelessResetTokens<'_> {}
impl Probing for crate::frame::MtuProbingComplete {}
impl Probing for crate::frame::HandshakeDone {}
impl Probing for crate::frame::MaxData {}
impl Probing for crate::frame::MaxStreamData {}
impl Probing for crate::frame::MaxStreams {}
impl Probing for crate::frame::NewConnectionId<'_> {
    #[inline]
    fn path_validation(&self) -> Probe {
        Probe::Probing
    }
}
impl Probing for crate::frame::NewToken<'_> {}
impl Probing for crate::frame::Padding {
    #[inline]
    fn path_validation(&self) -> Probe {
        Probe::Probing
    }
}
impl Probing for crate::frame::PathChallenge<'_> {
    #[inline]
    fn path_validation(&self) -> Probe {
        Probe::Probing
    }
}
impl Probing for crate::frame::PathResponse<'_> {
    #[inline]
    fn path_validation(&self) -> Probe {
        Probe::Probing
    }
}
impl Probing for crate::frame::Ping {}
impl Probing for crate::frame::ResetStream {}
impl Probing for crate::frame::RetireConnectionId {}
impl Probing for crate::frame::StopSending {}
impl<Data> Probing for crate::frame::Stream<Data> {}
impl Probing for crate::frame::StreamDataBlocked {}
impl Probing for crate::frame::StreamsBlocked {}

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.1
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

        assert!(probe.is_probing())
    }

    #[test]
    fn probing_and_non_probing_packet_test() {
        let mut probe = Probe::Probing;
        probe |= Probe::NonProbing;

        assert!(!probe.is_probing())
    }
}
