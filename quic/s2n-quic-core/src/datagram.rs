// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
pub trait Endpoint: 'static + Send {
    type Sender: Sender;
    type Receiver: Receiver;

    fn create_connection(&mut self, info: &ConnectionInfo) -> (Self::Sender, Self::Receiver);
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ConnectionInfo {}

pub trait Sender: 'static + Send {}
pub trait Receiver: 'static + Send {}

#[derive(Debug, Default)]
pub struct Disabled;

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (DisabledSender, DisabledReceiver)
    }
}
pub struct DisabledSender;
pub struct DisabledReceiver;

impl Receiver for DisabledReceiver {}
impl Sender for DisabledSender {}
