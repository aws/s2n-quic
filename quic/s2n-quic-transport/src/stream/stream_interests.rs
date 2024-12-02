// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A collection of a all the interactions a `Stream` is interested in

use crate::transmission::interest::{Interest, Query, QueryBreak, Result};

/// A collection of a all the interactions a `Stream` is interested in
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct StreamInterests {
    /// Is `true` if the `Stream` wants to transmit data but is blocked on
    /// insufficient connection flow control credits
    pub connection_flow_control_credits: bool,
    /// Is `true` if the `Stream` wants to transmit data but is blocked on
    /// insufficient stream flow control credits
    pub stream_flow_control_credits: bool,
    /// Is `true` if the `Stream` is still wanting to make progress. Otherwise
    /// the stream will be removed from the `Stream` map.
    pub retained: bool,
    /// Is `true` if the component is interested in packet acknowledge and
    /// loss information
    pub delivery_notifications: bool,
    /// Transmission interest for the component
    pub transmission: Interest,
}

impl StreamInterests {
    #[inline]
    pub fn merge(&mut self, other: &Self) {
        self.connection_flow_control_credits |= other.connection_flow_control_credits;
        self.stream_flow_control_credits |= other.stream_flow_control_credits;
        self.retained |= other.retained;
        self.delivery_notifications |= other.delivery_notifications;
        let _ = self.transmission.on_interest(other.transmission);
    }

    #[inline]
    pub fn with_transmission<F: FnOnce(&mut TransmissionInterest) -> Result>(&mut self, f: F) {
        let mut interest = TransmissionInterest(&mut self.transmission);
        let _ = f(&mut interest);
    }
}

pub struct TransmissionInterest<'a>(&'a mut Interest);

impl Query for TransmissionInterest<'_> {
    fn on_interest(&mut self, interest: Interest) -> Result {
        debug_assert_ne!(
            interest,
            Interest::Forced,
            "streams are not allowed to force transmission"
        );

        match (*self.0, interest) {
            // we don't need to keep querying if we're already at the max interest
            (Interest::LostData, _) | (_, Interest::LostData) => {
                *self.0 = Interest::LostData;
                return Err(QueryBreak);
            }
            (Interest::NewData, _) | (_, Interest::NewData) => *self.0 = Interest::NewData,
            _ => {}
        }

        Ok(())
    }
}

/// A type which can provide it's Stream interests
pub trait StreamInterestProvider {
    /// Returns all interactions the object is interested in.
    fn stream_interests(&self, interests: &mut StreamInterests);

    fn get_stream_interests(&self) -> StreamInterests {
        let mut interests = StreamInterests::default();
        self.stream_interests(&mut interests);
        interests
    }
}
