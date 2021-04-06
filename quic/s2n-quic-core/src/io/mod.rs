// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod rx;
pub mod tx;

/// A pair of Rx and Tx IO implementations
///
/// From https://en.wikipedia.org/wiki/Duplex_(telecommunications):
///
/// > A duplex communication system is a point-to-point system composed of two or more
/// > connected parties or devices that can communicate with one another in both directions.
#[derive(Debug)]
pub struct Duplex<Rx, Tx> {
    pub rx: Rx,
    pub tx: Tx,
}

impl<'a, Rx: rx::Rx<'a>, Tx> rx::Rx<'a> for Duplex<Rx, Tx> {
    type Queue = Rx::Queue;

    fn queue(&'a mut self) -> Self::Queue {
        self.rx.queue()
    }

    fn len(&self) -> usize {
        self.rx.len()
    }
}

impl<'a, Rx, Tx: tx::Tx<'a>> tx::Tx<'a> for Duplex<Rx, Tx> {
    type Queue = Tx::Queue;

    fn queue(&'a mut self) -> Self::Queue {
        self.tx.queue()
    }

    fn len(&self) -> usize {
        self.tx.len()
    }
}
