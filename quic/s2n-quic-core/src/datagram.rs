// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
pub trait DatagramApi: Sender + Receiver {}

impl<T: 'static + Sender + Receiver + Send> DatagramApi for T {}

pub trait Sender: 'static + Send {}
pub trait Receiver: 'static + Send {}

#[derive(Debug, Default)]
pub struct Disabled;

impl Receiver for Disabled {}
impl Sender for Disabled {}
