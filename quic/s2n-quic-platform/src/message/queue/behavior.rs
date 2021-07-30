// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message;

/// Trait which defines slice behavior on success and cancellation
pub trait Behavior {
    /// Advances the slice by count
    fn advance<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    );

    /// Returns `count` number of messages to their initial state
    fn cancel<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    );
}

/// Behavior for the Occupied Slice
///
/// After being successfully consumed an occupied message
/// should reset its payload_len to MTU.
///
/// Cancelling is a no-op.
#[derive(Debug)]
pub struct Occupied {
    pub(crate) mtu: usize,
}

impl Behavior for Occupied {
    fn advance<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    ) {
        reset(&mut primary[start..end], self.mtu);
        reset(&mut primary[..overflow], self.mtu);
        reset(&mut secondary[start..end], self.mtu);
        reset(&mut secondary[..overflow], self.mtu);
    }

    /// Cancelling an occupied message doesn't mutate any state
    fn cancel<Message: message::Message>(
        &self,
        _primary: &mut [Message],
        _secondary: &mut [Message],
        _start: usize,
        _end: usize,
        _overflow: usize,
    ) {
    }
}

/// Behavior for the Occupied Slice, with optional wiping
///
/// After being successfully consumed an occupied message
/// should reset its payload_len to MTU. Additionaly, if enabled
/// with the `wipe` feature, the payload will be wiped to prevent
/// sensitive data from persisting in memory.
///
/// Cancelling is a no-op.
#[derive(Debug)]
pub struct OccupiedWipe {
    pub(crate) mtu: usize,
}

impl Behavior for OccupiedWipe {
    fn advance<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    ) {
        // Because the primary and secondary messages point to the same
        // payloads in memory, only wiping the first is required
        wipe(&mut primary[start..end], self.mtu);
        wipe(&mut primary[..overflow], self.mtu);
        reset(&mut secondary[start..end], self.mtu);
        reset(&mut secondary[..overflow], self.mtu);
    }

    /// Cancelling an occupied message doesn't mutate any state
    fn cancel<Message: message::Message>(
        &self,
        _primary: &mut [Message],
        _secondary: &mut [Message],
        _start: usize,
        _end: usize,
        _overflow: usize,
    ) {
    }
}

/// Behavior of the Free Slice
///
/// After successfully writing to free messages, the fields need
/// to be replicated to their counterparts. This is to ensure all primary and
/// secondary messages are consistent.
///
/// Cancelling a slice of free messages will reset the messages to their
/// initial state, to ensure the full MTU can still be written.
#[derive(Debug)]
pub struct Free {
    pub(crate) mtu: usize,
}

impl Free {
    /// Replicates fields from one slice of Messages to another
    fn replicate<Message: message::Message>(&self, from: &mut [Message], to: &mut [Message]) {
        for (from_msg, to_msg) in from.iter_mut().zip(to.iter_mut()) {
            to_msg.replicate_fields_from(from_msg);
        }
    }
}

impl Behavior for Free {
    /// Replicates all of the fields that were modified to their counterpart
    fn advance<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    ) {
        self.replicate(&mut primary[start..end], &mut secondary[start..end]);
        self.replicate(&mut secondary[..overflow], &mut primary[..overflow]);
    }

    /// Cancelling a slice of free messages should reset to their initial state
    ///
    /// # Note
    /// Reset is called, instead of wipe, to reduce computational requirements.
    /// This assumes that no sensitive data has been written to the free slice,
    /// of if it has, the caller has manually wiped it. Otherwise, this will be
    /// wiping payloads that have already been wiped.
    ///
    /// If that assumption changes, this will need to be updated.
    fn cancel<Message: message::Message>(
        &self,
        primary: &mut [Message],
        secondary: &mut [Message],
        start: usize,
        end: usize,
        overflow: usize,
    ) {
        reset(&mut primary[start..end], self.mtu);
        reset(&mut primary[..overflow], self.mtu);
        reset(&mut secondary[start..end], self.mtu);
        reset(&mut secondary[..overflow], self.mtu);
    }
}

/// Resets all of the provided messages to an initial state
#[inline]
fn reset<Message: message::Message>(messages: &mut [Message], mtu: usize) {
    for message in messages {
        unsafe {
            // Safety: the payloads should always be allocated regions of MTU
            message.reset(mtu);
        }
    }
}

/// Wipes and resets all of the provided messages to an initial state
///
/// If the `wipe` feature is disabled, this behaves exactly like the reset
#[inline]
fn wipe<Message: message::Message>(messages: &mut [Message], mtu: usize) {
    for message in messages {
        // The payload could potentially contain sensitive data and should
        // be zeroed out in addition to reseting the state
        #[cfg(feature = "wipe")]
        zeroize::Zeroize::zeroize(&mut message.payload_mut().iter_mut());

        unsafe {
            // Safety: the payloads should always be allocated regions of MTU
            message.reset(mtu);
        }
    }
}
