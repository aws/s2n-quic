//! A collection of a all the interactions a `Stream` is interested in

use crate::frame_exchange_interests::FrameExchangeInterests;

/// A collection of a all the interactions a `Stream` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct StreamInterests {
    /// Is `true` if the `Stream` wants to transmit data but is blocked on
    /// insufficient connection flow control credits
    pub connection_flow_control_credits: bool,
    /// Is `true` if the `Stream` has entered it's final state and
    /// can therefore be removed from the `Stream` map.
    pub finalization: bool,
    /// Frame exchange related interests
    pub frame_exchange: FrameExchangeInterests,
}

impl StreamInterests {
    /// Merges 2 `StreamInterests` collections.
    ///
    /// For most interests, if at least one `StreamInterests` instance is
    /// interested in a certain interaction, the interest will be set on the
    /// returned `StreamInterests` instance.
    ///
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    ///
    /// The `finalization` interest is the exception. A `Stream` can only
    /// be finalized if both the sending and receiving side are interested
    /// in finalization.
    pub fn merge(self, other: StreamInterests) -> StreamInterests {
        StreamInterests {
            connection_flow_control_credits: self.connection_flow_control_credits
                || other.connection_flow_control_credits,
            finalization: self.finalization && other.finalization,
            frame_exchange: self.frame_exchange.merge(other.frame_exchange),
        }
    }

    /// Merges `FrameExchangeInterests` into `StreamInterest`s
    ///
    /// If at least one `StreamInterests` instance is interested in a certain
    /// interaction, the interest will be set on the returned `StreamInterests`
    /// instance.
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    pub fn merge_frame_exchange_interests(self, other: FrameExchangeInterests) -> StreamInterests {
        StreamInterests {
            connection_flow_control_credits: self.connection_flow_control_credits,
            finalization: self.finalization,
            frame_exchange: self.frame_exchange.merge(other),
        }
    }
}

// Overload the `+` and `+=` operator for `StreamInterests` to support merging
// multiple interest sets.

impl core::ops::Add for StreamInterests {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.merge(rhs)
    }
}

impl core::ops::Add<FrameExchangeInterests> for StreamInterests {
    type Output = Self;

    fn add(self, rhs: FrameExchangeInterests) -> Self::Output {
        self.merge_frame_exchange_interests(rhs)
    }
}

impl core::ops::AddAssign for StreamInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
    }
}

impl core::ops::AddAssign<FrameExchangeInterests> for StreamInterests {
    fn add_assign(&mut self, rhs: FrameExchangeInterests) {
        *self = self.merge_frame_exchange_interests(rhs);
    }
}

/// A type which can provide it's Stream interests
pub trait StreamInterestProvider {
    /// Returns all interactions the object is interested in.
    fn interests(&self) -> StreamInterests;
}
