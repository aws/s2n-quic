// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::TransportFeatures;

pub mod application;
#[cfg(any(test, feature = "testing"))]
mod bach;
pub mod fd;
mod handle;
mod send_only;
#[cfg(feature = "tokio")]
mod tokio;
mod tracing;

pub use self::tracing::Tracing;
pub use crate::socket::*;
pub use application::Application;
pub use handle::{Ext, Flags, Socket};
pub use send_only::SendOnly;

pub type ArcApplication = std::sync::Arc<dyn Application>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protocol {
    Tcp,
    Udp,
    Other(&'static str),
}

impl Protocol {
    s2n_quic_core::state::is!(is_tcp, Tcp);
    s2n_quic_core::state::is!(is_udp, Udp);

    #[inline]
    pub fn is_other(&self) -> bool {
        matches!(self, Self::Other(_))
    }
}
