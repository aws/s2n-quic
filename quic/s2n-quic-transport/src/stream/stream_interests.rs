//! A collection of a all the interactions a `Stream` is interested in

use crate::transmission;

/// A collection of a all the interactions a `Stream` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct StreamInterests {
    /// Is `true` if the `Stream` wants to transmit data but is blocked on
    /// insufficient connection flow control credits
    pub connection_flow_control_credits: bool,
    /// Is `true` if the `Stream` has entered it's final state and
    /// can therefore be removed from the `Stream` map.
    pub finalization: bool,
    pub delivery_notifications: bool,
    pub transmission: transmission::Interest,
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
            delivery_notifications: self.delivery_notifications || other.delivery_notifications,
            transmission: self.transmission + other.transmission,
        }
    }

    /// Merges `transmission::Interest` into `StreamInterest`s
    ///
    /// If at least one `StreamInterests` instance is interested in a certain
    /// interaction, the interest will be set on the returned `StreamInterests`
    /// instance.
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    pub fn merge_transmission_interest(self, other: transmission::Interest) -> StreamInterests {
        StreamInterests {
            transmission: self.transmission + other,
            ..self
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

impl core::ops::Add<transmission::Interest> for StreamInterests {
    type Output = Self;

    fn add(self, rhs: transmission::Interest) -> Self::Output {
        self.merge_transmission_interest(rhs)
    }
}

impl core::ops::AddAssign for StreamInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
    }
}

impl core::ops::AddAssign<transmission::Interest> for StreamInterests {
    fn add_assign(&mut self, rhs: transmission::Interest) {
        *self = self.merge_transmission_interest(rhs);
    }
}

/// A type which can provide it's Stream interests
pub trait StreamInterestProvider {
    /// Returns all interactions the object is interested in.
    fn interests(&self) -> StreamInterests;
}
