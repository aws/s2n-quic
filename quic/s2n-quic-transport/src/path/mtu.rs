// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::PacketNumber, path::MINIMUM_MTU, timer::VirtualTimer, transmission,
    transmission::Interest,
};
use s2n_quic_core::time::Timestamp;

//= https://tools.ietf.org/rfc/rfc8899.txt#5.2
//#    |         |
//#    | Start   | PL indicates loss
//#    |         |  of connectivity
//#    v         v
//# +---------------+                                   +---------------+
//# |    DISABLED   |                                   |     ERROR     |
//# +---------------+               PROBE_TIMER expiry: +---------------+
//#         | PL indicates     PROBE_COUNT = MAX_PROBES or    ^      |
//#         | connectivity  PTB: PL_PTB_SIZE < BASE_PLPMTU    |      |
//#         +--------------------+         +------------------+      |
//#                              |         |                         |
//#                              v         |       BASE_PLPMTU Probe |
//#                           +---------------+          acked       |
//#                           |      BASE     |--------------------->+
//#                           +---------------+                      |
//#                              ^ |    ^  ^                         |
//#          Black hole detected | |    |  | Black hole detected     |
//#         +--------------------+ |    |  +--------------------+    |
//#         |                      +----+                       |    |
//#         |                PROBE_TIMER expiry:                |    |
//#         |             PROBE_COUNT < MAX_PROBES              |    |
//#         |                                                   |    |
//#         |               PMTU_RAISE_TIMER expiry             |    |
//#         |    +-----------------------------------------+    |    |
//#         |    |                                         |    |    |
//#         |    |                                         v    |    v
//# +---------------+                                   +---------------+
//# |SEARCH_COMPLETE|                                   |   SEARCHING   |
//# +---------------+                                   +---------------+
//#    |    ^    ^                                         |    |    ^
//#    |    |    |                                         |    |    |
//#    |    |    +-----------------------------------------+    |    |
//#    |    |            MAX_PLPMTU Probe acked or              |    |
//#    |    |  PROBE_TIMER expiry: PROBE_COUNT = MAX_PROBES or  |    |
//#    +----+            PTB: PL_PTB_SIZE = PLPMTU              +----+
//# CONFIRMATION_TIMER expiry:                        PROBE_TIMER expiry:
//# PROBE_COUNT < MAX_PROBES or               PROBE_COUNT < MAX_PROBES or
//#      PLPMTU Probe acked                           Probe acked or PTB:
//#                                    PLPMTU < PL_PTB_SIZE < PROBED_SIZE
#[derive(Debug, PartialEq, Eq)]
enum State {
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The DISABLED state is the initial state before probing has started.
    Disabled,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The BASE state is used to confirm that the BASE_PLPMTU size is
    //# supported by the network path and is designed to allow an
    //# application to continue working when there are transient reductions
    //# in the actual PMTU.
    Base,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCHING state is the main probing state.
    Searching(PacketNumber),
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCH_COMPLETE state indicates that a search has completed.
    SearchComplete,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The ERROR state represents the case where either the network path
    //# is not known to support a PLPMTU of at least the BASE_PLPMTU size
    //# or when there is contradictory information about the network path
    //# that would otherwise result in excessive variation in the MPS
    //# signaled to the higher layer.
    Error,
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.3
//# Endpoints SHOULD set the initial value of BASE_PLPMTU (Section 5.1 of
//# [DPLPMTUD]) to be consistent with QUIC's smallest allowed maximum
//# datagram size.

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# When using
//# IPv4, there is no currently equivalent size specified, and a
//# default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
const BASE_PLPMTU: u16 = MINIMUM_MTU;

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# The MAX_PROBES is the maximum value of the PROBE_COUNT
//# counter (see Section 5.1.3).  MAX_PROBES represents the limit for
//# the number of consecutive probe attempts of any size.  Search
//# algorithms benefit from a MAX_PROBES value greater than 1 because
//# this can provide robustness to isolated packet loss.  The default
//# value of MAX_PROBES is 3.
const MAX_PROBES: u8 = 3;

pub struct Controller {
    state: State,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBE_COUNT is a count of the number of successive
    //# unsuccessful probe packets that have been sent.
    probe_count: u8,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBED_SIZE is the size of the current probe packet
    //# as determined at the PL.  This is a tentative value for the
    //# PLPMTU, which is awaiting confirmation by an acknowledgment.
    probed_size: u16,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.1
    //# The PROBE_TIMER is configured to expire after a period
    //# longer than the maximum time to receive an acknowledgment to a
    //# probe packet.
    probe_timer: VirtualTimer,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.1
    //# The PMTU_RAISE_TIMER is configured to the period a
    //# sender will continue to use the current PLPMTU, after which it
    //# reenters the Search Phase.
    pmtu_raise_timer: VirtualTimer,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            state: State::Disabled,
            probe_count: 0,
            probed_size: 1450, // TODO: use correct value
            probe_timer: VirtualTimer::default(),
            pmtu_raise_timer: VirtualTimer::default(),
        }
    }

    pub fn enable(&mut self) {
        debug_assert_eq!(self.state, State::Disabled);

        // TODO: Look up current MTU in the cache. If there is a cache hit
        //       move directly to SearchComplete. Otherwise, we should start searching
        //       for a larger PMTU immediately
        self.state = State::Base;
    }

    /// Returns all timers for the component
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        core::iter::empty()
            .chain(self.probe_timer.iter())
            .chain(self.pmtu_raise_timer.iter())
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.probe_timer.poll_expiration(now).is_ready() {
            self.probe_count += 1;
            if self.probe_count < MAX_PROBES {
                // send another probe
            } else {
                self.state = State::SearchComplete;
            }
        }

        if self.pmtu_raise_timer.poll_expiration(now).is_ready() {
            // send another probe
        }
    }
}

// In Base or
impl transmission::interest::Provider for Controller {
    fn transmission_interest(&self) -> Interest {
        todo!()
    }
}
