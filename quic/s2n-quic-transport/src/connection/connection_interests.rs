//! A collection of a all the interactions a `Connection` is interested in

/// A collection of a all the interactions a `Connection` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct ConnectionInterests {
    /// Is `true` if the `Connection` has entered it's final state and
    /// can therefore be removed from the `Connection` map.
    pub finalization: bool,
    /// Is `true` if a `Connection` completed the handshake and should be transferred
    /// to the application via an accept call.
    pub accept: bool,
    /// Is `true` if a `Connection` wants to send data
    pub transmission: bool,
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
            transmission: self.transmission || other.transmission,
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

impl core::ops::AddAssign for ConnectionInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
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
            },
            accept: true,
            finalization: true,
        };

        let b = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: true,
                delivery_notifications: false,
            },
            accept: false,
            finalization: false,
        };

        let c = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: false,
                delivery_notifications: false,
            },
            accept: false,
            finalization: true,
        };

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: true,
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
            },
            accept: true,
            finalization: false,
        };

        let b = FrameExchangeInterests {
            transmission: true,
            delivery_notifications: false,
        };

        let c = FrameExchangeInterests {
            transmission: false,
            delivery_notifications: false,
        };

        assert_eq!(
            ConnectionInterests {
                frame_exchange: FrameExchangeInterests {
                    transmission: true,
                    delivery_notifications: true,
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
            },
            finalization: false,
            accept: true,
        };

        let b = ConnectionInterests {
            frame_exchange: FrameExchangeInterests {
                transmission: true,
                delivery_notifications: false,
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
                },
                finalization: true,
                accept: false,
            },
            b.merge_stream_manager_interests(s2)
        );
    }
}
