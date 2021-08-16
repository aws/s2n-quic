// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{inet::datagram, path};

/// A structure capable of queueing and receiving messages
pub trait Queue {
    type Entry: Entry<Handle = Self::Handle>;
    type Handle: path::Handle;

    /// Returns a slice of all of the entries in the queue
    fn as_slice_mut(&mut self) -> &mut [Self::Entry];

    /// Returns the number of items in the queue
    fn len(&self) -> usize;

    /// Returns `true` if the queue is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Consumes `count` number of entries in the queue
    fn finish(&mut self, count: usize);
}

/// An entry in a Rx queue
pub trait Entry {
    type Handle: path::Handle;

    /// Returns the datagram information with the datagram payload
    fn read(&mut self) -> Option<(datagram::Header<Self::Handle>, &mut [u8])>;
}
