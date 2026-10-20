// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use std::sync::Arc;

pub mod builder;

pub use builder::Builder;

pub trait Application: 'static + Send + Sync {
    /// Creates application sockets registered with the current Tokio runtime.
    #[cfg(feature = "tokio")]
    fn rebind_to_current_runtime(&self) -> std::io::Result<super::ArcApplication> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "application sockets do not support runtime rebinding",
        ))
    }

    fn protocol(&self) -> Protocol;

    fn features(&self) -> TransportFeatures;

    fn write_application(&self) -> &dyn Socket;

    fn read_application(&self) -> &dyn Socket;
}

impl<T: ?Sized + Application> Application for Arc<T> {
    #[cfg(feature = "tokio")]
    fn rebind_to_current_runtime(&self) -> std::io::Result<super::ArcApplication> {
        (**self).rebind_to_current_runtime()
    }

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

pub struct Single<S>(pub(crate) S);

impl<S: Socket> Application for Single<S> {
    #[cfg(feature = "tokio")]
    fn rebind_to_current_runtime(&self) -> std::io::Result<super::ArcApplication> {
        Ok(Arc::new(Single(self.0.rebind_to_current_runtime()?)))
    }

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
    #[cfg(feature = "tokio")]
    fn rebind_to_current_runtime(&self) -> std::io::Result<super::ArcApplication> {
        let read = self.read.rebind_to_current_runtime()?;
        let write = self.write.rebind_to_current_runtime()?;
        Ok(Arc::new(Pair { read, write }))
    }

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
