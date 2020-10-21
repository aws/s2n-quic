//! A collection of a all the interactions a component which is interested to
//! send or receive Frames can be interested in

/// A collection of a all the interactions a component which is interested to
/// send or receive Frames can be interested in.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct FrameExchangeInterests {
    /// Is `true` if the component is interested in packet acknowledge and
    /// loss information
    pub delivery_notifications: bool,
    /// Is `true` if the component is interested in transmitting outgoing data
    pub transmission: bool,
    /// Is `true` if the frame must be transmitted regardless of congestion control window limits
    pub ignore_congestion_control: bool,
}

impl FrameExchangeInterests {
    /// Merges two `FrameExchangeInterests` collections.
    ///
    /// If at least one `FrameExchangeInterests` instance is interested in a certain
    /// interaction, the interest will be set on the returned `FrameExchangeInterests`
    /// instance.
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    pub fn merge(self, other: FrameExchangeInterests) -> FrameExchangeInterests {
        FrameExchangeInterests {
            delivery_notifications: self.delivery_notifications || other.delivery_notifications,
            transmission: self.transmission || other.transmission,
            ignore_congestion_control: self.ignore_congestion_control
                || other.ignore_congestion_control,
        }
    }
}

// Overload the `+` and `+=` operator for `FrameExchangeInterests` to support
// merging multiple interest sets.

impl core::ops::Add for FrameExchangeInterests {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.merge(rhs)
    }
}

impl core::ops::AddAssign for FrameExchangeInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
    }
}

/// A type which can provide it's frame exchange interests
pub trait FrameExchangeInterestProvider {
    /// Returns all interactions the object is interested in.
    fn frame_exchange_interests(&self) -> FrameExchangeInterests;
}
