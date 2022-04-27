// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod interest {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Interest {
        None,
        #[allow(dead_code)]
        Immediate,
    }

    impl Interest {
        #[inline]
        pub fn is_none(self) -> bool {
            matches!(self, Interest::None)
        }
    }

    pub trait Provider {
        fn ack_interest<Q: Query>(&self, query: &mut Q) -> Result;

        #[inline]
        fn has_ack_interest(&self) -> bool {
            let mut query = HasAckInterestQuery;
            self.ack_interest(&mut query).is_err()
        }
    }

    pub trait Query {
        fn on_interest(&mut self, interest: Interest) -> Result;
    }

    pub struct HasAckInterestQuery;

    impl Query for HasAckInterestQuery {
        #[inline]
        fn on_interest(&mut self, interest: Interest) -> Result {
            if interest.is_none() {
                Ok(())
            } else {
                // If we've got anything other than `None` then bail since we now have an answer
                Err(QueryBreak)
            }
        }
    }

    pub struct QueryBreak;

    pub type Result<T = (), E = QueryBreak> = core::result::Result<T, E>;
}
