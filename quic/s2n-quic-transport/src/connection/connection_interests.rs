//! A collection of a all the interactions a `Connection` is interested in

use crate::{frame_exchange_interests::FrameExchangeInterests, stream::StreamManagerInterests};

/// A collection of a all the interactions a `Connection` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct ConnectionInterests {
    /// Is `true` if the `Connection` has entered it's final state and
    /// can therefore be removed from the `Connection` map.
    pub finalization: bool,
    /// Is `true` if a `Connection` completed the handshake and should be transferred
    /// to the application via an accept call.
    pub accept: bool,
    /// Frame exchange related interests
    pub frame_exchange: FrameExchangeInterests,
}

impl ConnectionInterests {
    /// Merges 2 `ConnectionInterests` collections.
    ///
    /// For most interests, if at least one `ConnectionInterests` instance is
    /// interested in a certain interaction, the interest will be set on the
    /// returned `ConnectionInterests` instance.
    ///
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    ///
    /// The `finalization` interest is the exception. A `Connection` can only
    /// be finalized if all parts are interested in finalization.
    pub fn merge(self, other: ConnectionInterests) -> ConnectionInterests {
        ConnectionInterests {
            finalization: self.finalization && other.finalization,
            accept: self.accept || other.accept,
            frame_exchange: self.frame_exchange.merge(other.frame_exchange),
        }
    }

    /// Merges `FrameExchangeInterests` into `ConnectionInterest`s
    ///
    /// If at least one `ConnectionInterests` instance is interested in a certain
    /// interaction, the interest will be set on the returned `ConnectionInterests`
    /// instance.
    ///
    /// Thereby the operation performs a field-wise logical `OR`
    pub fn merge_frame_exchange_interests(
        self,
        other: FrameExchangeInterests,
    ) -> ConnectionInterests {
        ConnectionInterests {
            finalization: self.finalization,
            accept: self.accept,
            frame_exchange: self.frame_exchange.merge(other),
        }
    }

    /// Merges `StreamManagerInterest`s into `ConnectionInterest`s
    ///
    /// If at least one instance is interested in a certain interaction,
    /// the interest will be set on the returned `ConnectionInterests` instance.
    ///
    /// Thereby the operation performs a field-wise logical `OR`.
    ///
    /// The `finalization` interest is an exception: It will only set to `true`
    /// if all components are interested in `finalization`.
    pub fn merge_stream_manager_interests(
        self,
        other: StreamManagerInterests,
    ) -> ConnectionInterests {
        ConnectionInterests {
            finalization: self.finalization && other.finalization,
            accept: self.accept,
            frame_exchange: FrameExchangeInterests {
                transmission: self.frame_exchange.transmission || other.transmission,
                delivery_notifications: self.frame_exchange.delivery_notifications,
                ignore_congestion_control: self.frame_exchange.ignore_congestion_control,
            },
        }
    }
}

// Overload the `+` and `+=` operator for `ConnectionInterests` to support merging
// multiple interest sets.

impl core::ops::Add for ConnectionInterests {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.merge(rhs)
    }
}

impl core::ops::Add<FrameExchangeInterests> for ConnectionInterests {
    type Output = Self;

    fn add(self, rhs: FrameExchangeInterests) -> Self::Output {
        self.merge_frame_exchange_interests(rhs)
    }
}

impl core::ops::Add<StreamManagerInterests> for ConnectionInterests {
    type Output = Self;

    fn add(self, rhs: StreamManagerInterests) -> Self::Output {
        self.merge_stream_manager_interests(rhs)
    }
}

impl core::ops::AddAssign for ConnectionInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
    }
}

impl core::ops::AddAssign<FrameExchangeInterests> for ConnectionInterests {
    fn add_assign(&mut self, rhs: FrameExchangeInterests) {
        *self = self.merge_frame_exchange_interests(rhs);
    }
}

impl core::ops::AddAssign<StreamManagerInterests> for ConnectionInterests {
    fn add_assign(&mut self, rhs: StreamManagerInterests) {
        *self = self.merge_stream_manager_interests(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_connection_interests() {
        let a = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: false,
                delivery_notifications: true,
                ignore_congestion_control: false,
            },
            accept: true,
            finalization: true,
        };

        let b = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: true,
                delivery_notifications: false,
                ignore_congestion_control: false,
            },
            accept: false,
            finalization: false,
        };

        let c = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: false,
                delivery_notifications: false,
                ignore_congestion_control: false,
            },
            accept: false,
            finalization: true,
        };

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                accept: true,
                finalization: false,
            },
            a + b
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: false,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                accept: true,
                finalization: true,
            },
            a + c
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: false,
                    ignore_congestion_control: false,
                },
                accept: false,
                finalization: false,
            },
            b + c
        );
    }

    #[test]
    fn test_merge_frame_exchange_interests() {
        let a = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: false,
                delivery_notifications: true,
                ignore_congestion_control: false,
            },
            accept: true,
            finalization: false,
        };

        let b = FrameExchangeInterests {
            transmission: true,
            delivery_notifications: false,
            ignore_congestion_control: false,
        };

        let c = FrameExchangeInterests {
            transmission: false,
            delivery_notifications: false,
            ignore_congestion_control: false,
        };

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                accept: true,
                finalization: false,
            },
            a.merge_frame_exchange_interests(b)
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: false,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                accept: true,
                finalization: false,
            },
            a.merge_frame_exchange_interests(c)
        );
    }

    #[test]
    fn test_merge_stream_manager_interests() {
        let a = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: false,
                delivery_notifications: true,
                ignore_congestion_control: false,
            },
            finalization: false,
            accept: true,
        };

        let b = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: true,
                delivery_notifications: false,
                ignore_congestion_control: false,
            },
            finalization: true,
            accept: false,
        };

        let s1 = StreamManagerInterests {
            transmission: true,
            finalization: false,
        };

        let s2 = StreamManagerInterests {
            transmission: false,
            finalization: true,
        };

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                finalization: false,
                accept: true,
            },
            a.merge_stream_manager_interests(s1)
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: false,
                    delivery_notifications: true,
                    ignore_congestion_control: false,
                },
                finalization: false,
                accept: true,
            },
            a.merge_stream_manager_interests(s2)
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: false,
                    ignore_congestion_control: false,
                },
                finalization: false,
                accept: false,
            },
            b.merge_stream_manager_interests(s1)
        );

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: false,
                    ignore_congestion_control: false,
                },
                finalization: true,
                accept: false,
            },
            b.merge_stream_manager_interests(s2)
        );
    }
}
