// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, stream::application::Builder as StreamBuilder, sync::mpmc as channel};

#[derive(Clone, Copy, Default)]
pub enum Flavor {
    #[default]
    Fifo,
    Lifo,
}

pub type Sender<Sub> = channel::Sender<StreamBuilder<Sub>>;
pub type Receiver<Sub> = channel::Receiver<StreamBuilder<Sub>>;

#[inline]
pub fn channel<Sub>(capacity: usize) -> (Sender<Sub>, Receiver<Sub>)
where
    Sub: event::Subscriber,
{
    channel::new(capacity)
}
