// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
    /// Is `true` if a `Connection` needs a new connection id
    pub new_connection_id: bool,
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
            new_connection_id: self.new_connection_id || other.new_connection_id,
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
            transmission: false,
            accept: true,
            finalization: true,
            new_connection_id: false,
        };

        let b = ConnectionInterests {
            transmission: true,
            accept: false,
            finalization: false,
            new_connection_id: true,
        };

        let c = ConnectionInterests {
            transmission: false,
            accept: false,
            finalization: true,
            new_connection_id: false,
        };

        assert_eq!(
            ConnectionInterests {
                transmission: true,
                accept: true,
                finalization: false,
                new_connection_id: true,
            },
            a + b
        );

        assert_eq!(
            ConnectionInterests {
                transmission: false,
                accept: true,
                finalization: true,
                new_connection_id: false,
            },
            a + c
        );

        assert_eq!(
            ConnectionInterests {
                transmission: true,
                accept: false,
                finalization: false,
                new_connection_id: true,
            },
            b + c
        );
    }
}
