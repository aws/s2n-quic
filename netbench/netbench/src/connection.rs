// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use core::task::{Context, Poll};

pub trait Connection {
    fn id(&self) -> u64;
    fn poll_open_bidirectional_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>>;
    fn poll_open_send_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>>;
    fn poll_accept_stream(&mut self, cx: &mut Context) -> Poll<Result<Option<u64>>>;
    fn poll_send(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>>;
    fn poll_receive(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>>;
    fn poll_send_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>>;
    fn poll_receive_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>>;
    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        let _ = cx;
        Ok(()).into()
    }
    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        let _ = cx;
        Ok(()).into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Local,
    Remote,
}

impl<T> core::ops::Index<Owner> for [T; 2] {
    type Output = T;

    fn index(&self, index: Owner) -> &Self::Output {
        match index {
            Owner::Local => &self[0],
            Owner::Remote => &self[1],
        }
    }
}

impl<T> core::ops::IndexMut<Owner> for [T; 2] {
    fn index_mut(&mut self, index: Owner) -> &mut Self::Output {
        match index {
            Owner::Local => &mut self[0],
            Owner::Remote => &mut self[1],
        }
    }
}

impl core::ops::Not for Owner {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }
}
