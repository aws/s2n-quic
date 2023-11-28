// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::transmission::Constraint;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interest {
    None,
    NewData,
    LostData,
    Forced,
}

impl Default for Interest {
    #[inline]
    fn default() -> Self {
        Self::None
    }
}

impl Interest {
    #[inline]
    pub fn can_transmit(self, limit: Constraint) -> bool {
        match (self, limit) {
            // nothing can be transmitted when we're at amplification limits
            (_, Constraint::AmplificationLimited) => false,

            // a component wants to try to recover so ignore limits
            (Interest::Forced, _) => true,

            // transmit lost data when we're either not limited, probing, or we want to do a fast
            // retransmission to try to recover
            (Interest::LostData, _) => limit.can_retransmit(),

            // new data may only be transmitted when we're not limited or probing
            (Interest::NewData, _) => limit.can_transmit(),

            // nothing is interested in transmitting anything
            (Interest::None, _) => false,
        }
    }

    #[inline]
    pub fn is_none(self) -> bool {
        matches!(self, Interest::None)
    }

    #[inline]
    pub fn merge_with<F: FnOnce(&mut Self) -> Result>(&mut self, f: F) -> Result {
        f(self)
    }
}

pub trait Provider {
    fn transmission_interest<Q: Query>(&self, query: &mut Q) -> Result;

    #[inline]
    fn get_transmission_interest(&self) -> Interest {
        let mut interest = Interest::None;
        let _ = self.transmission_interest(&mut interest);
        interest
    }

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        let mut query = HasTransmissionInterestQuery;
        self.transmission_interest(&mut query).is_err()
    }

    #[inline]
    fn can_transmit(&self, mut constraint: Constraint) -> bool {
        self.transmission_interest(&mut constraint).is_err()
    }
}

pub trait Query {
    fn on_interest(&mut self, interest: Interest) -> Result;

    #[inline]
    fn on_new_data(&mut self) -> Result {
        self.on_interest(Interest::NewData)
    }

    #[inline]
    fn on_lost_data(&mut self) -> Result {
        self.on_interest(Interest::LostData)
    }

    #[inline]
    fn on_forced(&mut self) -> Result {
        self.on_interest(Interest::Forced)
    }
}

impl Query for Interest {
    #[inline]
    fn on_interest(&mut self, interest: Interest) -> Result {
        match (*self, interest) {
            // we don't need to keep querying if we're already at the max interest
            (Interest::Forced, _) | (_, Interest::Forced) => {
                *self = Interest::Forced;
                return Err(QueryBreak);
            }
            (Interest::LostData, _) | (_, Interest::LostData) => *self = Interest::LostData,
            (Interest::NewData, _) | (_, Interest::NewData) => *self = Interest::NewData,
            (Interest::None, _) => {}
        }

        Ok(())
    }
}

impl Query for Constraint {
    #[inline]
    fn on_interest(&mut self, interest: Interest) -> Result {
        // If we can transmit with the given constraint bail since we now have an answer
        if interest.can_transmit(*self) {
            return Err(QueryBreak);
        }

        Ok(())
    }
}

pub struct HasTransmissionInterestQuery;

impl Query for HasTransmissionInterestQuery {
    #[inline]
    fn on_interest(&mut self, interest: Interest) -> Result {
        if interest.is_none() {
            Ok(())
        } else {
            // If we've got anything other than `None` then bail since we now have an answer
            Err(QueryBreak)
        }
    }

    // any calls to interest should bail the query
    #[inline]
    fn on_new_data(&mut self) -> Result {
        Err(QueryBreak)
    }

    #[inline]
    fn on_lost_data(&mut self) -> Result {
        Err(QueryBreak)
    }

    #[inline]
    fn on_forced(&mut self) -> Result {
        Err(QueryBreak)
    }
}

#[cfg(feature = "std")]
pub struct Debugger;

#[cfg(feature = "std")]
impl Query for Debugger {
    #[inline]
    #[track_caller]
    fn on_interest(&mut self, interest: Interest) -> Result {
        eprintln!("  {} - {:?}", core::panic::Location::caller(), interest);
        Ok(())
    }

    #[inline]
    #[track_caller]
    fn on_new_data(&mut self) -> Result {
        eprintln!(
            "  {} - {:?}",
            core::panic::Location::caller(),
            Interest::NewData
        );
        Ok(())
    }

    #[inline]
    #[track_caller]
    fn on_lost_data(&mut self) -> Result {
        eprintln!(
            "  {} - {:?}",
            core::panic::Location::caller(),
            Interest::LostData
        );
        Ok(())
    }

    #[inline]
    #[track_caller]
    fn on_forced(&mut self) -> Result {
        eprintln!(
            "  {} - {:?}",
            core::panic::Location::caller(),
            Interest::Forced
        );
        Ok(())
    }
}

pub struct QueryBreak;

pub type Result<T = (), E = QueryBreak> = core::result::Result<T, E>;

#[cfg(test)]
mod test {
    use crate::transmission::{
        interest::Query,
        Constraint,
        Constraint::*,
        Interest::{None, *},
    };

    #[test]
    fn ordering_test() {
        assert!(None < NewData);
        assert!(NewData < LostData);
        assert!(LostData < Forced);
    }

    #[test]
    fn interest_query_test() {
        let levels = [None, NewData, LostData, Forced];
        for a in levels.iter().copied() {
            for b in levels.iter().copied() {
                let mut query = a;
                let result = query.on_interest(b);

                assert_eq!(query, a.max(b));
                assert_eq!(matches!(a, Forced) || matches!(b, Forced), result.is_err());
            }
        }
    }

    #[test]
    fn can_transmit() {
        // Amplification Limited
        assert!(!None.can_transmit(AmplificationLimited));
        assert!(!NewData.can_transmit(AmplificationLimited));
        assert!(!LostData.can_transmit(AmplificationLimited));
        assert!(!Forced.can_transmit(AmplificationLimited));

        // Congestion Limited
        assert!(!None.can_transmit(CongestionLimited));
        assert!(!NewData.can_transmit(CongestionLimited));
        assert!(!LostData.can_transmit(CongestionLimited));
        assert!(Forced.can_transmit(CongestionLimited));

        // Retransmission Only
        assert!(!None.can_transmit(RetransmissionOnly));
        assert!(!NewData.can_transmit(RetransmissionOnly));
        assert!(LostData.can_transmit(RetransmissionOnly));
        assert!(Forced.can_transmit(RetransmissionOnly));

        // No Constraint
        assert!(!None.can_transmit(Constraint::None));
        assert!(NewData.can_transmit(Constraint::None));
        assert!(LostData.can_transmit(Constraint::None));
        assert!(Forced.can_transmit(Constraint::None));
    }
}
