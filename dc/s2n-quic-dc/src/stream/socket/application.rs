// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use std::sync::Arc;

pub mod builder;

pub use builder::Builder;

pub trait Application: 'static + Send + Sync {
    fn protocol(&self) -> Protocol;

    fn features(&self) -> TransportFeatures;

    fn write_application(&self) -> &dyn Socket;

    fn read_application(&self) -> &dyn Socket;
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
    fn write_application(&self) -> &dyn Socket {
        (**self).write_application()
    }

    #[inline]
    fn read_application(&self) -> &dyn Socket {
        (**self).read_application()
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
    fn write_application(&self) -> &dyn Socket {
        &self.0
    }

    #[inline]
    fn read_application(&self) -> &dyn Socket {
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
    fn write_application(&self) -> &dyn Socket {
        &self.write
    }

    #[inline]
    fn read_application(&self) -> &dyn Socket {
        &self.read
    }
}
