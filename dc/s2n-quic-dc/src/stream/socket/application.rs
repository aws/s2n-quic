// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use std::sync::Arc;

pub mod builder;

pub use builder::Builder;

pub use super::Socket as ReaderRecv;

/*
pub trait ReaderRecv {
    fn local_addr(&self) -> std::io::Result<std::net::SocketAddr>;

    fn features(&self) -> TransportFeatures;
}
    */

pub trait Application: 'static + Send + Sync {
    fn protocol(&self) -> Protocol;

    fn features(&self) -> TransportFeatures;

    /// Used to send application data
    fn write_application_sender(&self) -> &dyn Socket;

    /// Used to send ACKs
    fn read_application_sender(&self) -> &dyn Socket;

    /// Used to receive application data
    fn read_application_receiver(&self) -> &dyn ReaderRecv;
}

impl<T: ?Sized + Application> Application for Arc<T> {
    #[inline]
    fn protocol(&self) -> Protocol {
        (**self).protocol()
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        (**self).features()
    }

    #[inline]
    fn write_application_sender(&self) -> &dyn Socket {
        (**self).write_application_sender()
    }

    #[inline]
    fn read_application_sender(&self) -> &dyn Socket {
        (**self).read_application_sender()
    }

    #[inline]
    fn read_application_receiver(&self) -> &dyn Socket {
        (**self).read_application_receiver()
    }
}

pub struct Single<S: Socket>(S);

impl<S: Socket> Application for Single<S> {
    #[inline]
    fn protocol(&self) -> Protocol {
        self.0.protocol()
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        self.0.features()
    }

    #[inline]
    fn write_application_sender(&self) -> &dyn Socket {
        &self.0
    }

    #[inline]
    fn read_application_sender(&self) -> &dyn Socket {
        &self.0
    }

    #[inline]
    fn read_application_receiver(&self) -> &dyn Socket {
        &self.0
    }
}

pub struct Pair<S: Socket> {
    read: S,
    write: S,
}

impl<S: Socket> Application for Pair<S> {
    #[inline]
    fn protocol(&self) -> Protocol {
        self.read.protocol()
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        self.read.features()
    }

    #[inline]
    fn write_application_sender(&self) -> &dyn Socket {
        &self.write
    }

    #[inline]
    fn read_application_sender(&self) -> &dyn Socket {
        &self.read
    }

    #[inline]
    fn read_application_receiver(&self) -> &dyn Socket {
        &self.read
    }
}
