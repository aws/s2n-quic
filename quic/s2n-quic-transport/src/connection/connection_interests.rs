// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A collection of a all the interactions a `Connection` is interested in

use s2n_quic_core::time::Timestamp;

/// A collection of a all the interactions a `Connection` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct ConnectionInterests {
    /// Is `true` if the `Connection` has entered it's final state and
    /// can therefore be removed from the `Connection` map.
    pub finalization: bool,
    /// Is `true` if the `Connection` has entered the closing state and
    /// shared state should be freed
    pub closing: bool,
    /// Is `true` if a `Connection` completed the handshake and should be transferred
    /// to the application via an accept call.
    pub accept: bool,
    /// Is `true` if a `Connection` wants to send data
    pub transmission: bool,
    /// Is `true` if a `Connection` needs a new connection id
    pub new_connection_id: bool,
    /// Is `Some(Timestamp)` if the connection needs to be woken up at the specified time
    pub timeout: Option<Timestamp>,
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
            closing: self.closing && other.closing,
            accept: self.accept || other.accept,
            transmission: self.transmission || other.transmission,
            new_connection_id: self.new_connection_id || other.new_connection_id,
            timeout: match (self.timeout, other.timeout) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
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

impl core::ops::AddAssign for ConnectionInterests {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.merge(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn test_merge_connection_interests() {
        let a = ConnectionInterests {
            transmission: false,
            accept: true,
            finalization: true,
            closing: true,
            new_connection_id: false,
            timeout: None,
        };

        let b_time = unsafe { Timestamp::from_duration(Duration::from_secs(123)) };
        let b = ConnectionInterests {
            transmission: true,
            accept: false,
            finalization: false,
            closing: false,
            new_connection_id: true,
            timeout: Some(b_time),
        };

        let c_time = unsafe { Timestamp::from_duration(Duration::from_secs(456)) };
        let c = ConnectionInterests {
            transmission: false,
            accept: false,
            finalization: true,
            closing: true,
            new_connection_id: false,
            timeout: Some(c_time),
        };

        assert_eq!(
            ConnectionInterests {
                transmission: true,
                accept: true,
                finalization: false,
                closing: false,
                new_connection_id: true,
                timeout: Some(b_time),
            },
            a + b
        );

        assert_eq!(
            ConnectionInterests {
                transmission: false,
                accept: true,
                finalization: true,
                closing: true,
                new_connection_id: false,
                timeout: Some(c_time),
            },
            a + c
        );

        assert_eq!(
            ConnectionInterests {
                transmission: true,
                accept: false,
                finalization: false,
                closing: false,
                new_connection_id: true,
                timeout: Some(b_time),
            },
            b + c
        );
    }
}
