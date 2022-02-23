// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::marker::PhantomData;

#[derive(Debug)]
pub struct Checkpoint<Endpoint, Location, Op> {
    pub(crate) id: u64,
    endpoint: PhantomData<Endpoint>,
    location: PhantomData<Location>,
    op: PhantomData<Op>,
}

impl<Endpoint, Location, Op> Checkpoint<Endpoint, Location, Op> {
    pub(crate) fn new(id: u64) -> Self {
        Self {
            id,
            endpoint: PhantomData,
            location: PhantomData,
            op: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Park;

#[derive(Debug)]
pub struct Unpark;

macro_rules! sync {
    ($endpoint:ty, $location:ty) => {
        pub fn park(
            &mut self,
            checkpoint: crate::scenario::builder::checkpoint::Checkpoint<
                $endpoint,
                $location,
                crate::scenario::builder::checkpoint::Park,
            >,
        ) -> &mut Self {
            self.ops.push(crate::operation::Connection::Park {
                checkpoint: checkpoint.id,
            });
            self
        }

        pub fn unpark(
            &mut self,
            checkpoint: crate::scenario::builder::checkpoint::Checkpoint<
                $endpoint,
                $location,
                crate::scenario::builder::checkpoint::Unpark,
            >,
        ) -> &mut Self {
            self.ops.push(crate::operation::Connection::Unpark {
                checkpoint: checkpoint.id,
            });
            self
        }
    };
}
