// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, panic::Location};

/// For errors produced during client connection setup.
#[derive(Clone, Copy)]
pub struct Error {
    pub(crate) kind: Kind,
    pub(crate) location: &'static Location<'static>,
}

impl Error {
    #[track_caller]
    #[inline]
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            location: Location::caller(),
        }
    }

    #[inline]
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// Recovers a connect [`Error`] from an [`std::io::Error`], if it originated as one.
    #[inline]
    pub fn from_io(err: &std::io::Error) -> Option<&Self> {
        err.get_ref().and_then(|inner| inner.downcast_ref::<Self>())
    }

    #[inline]
    fn file(&self) -> &'static str {
        self.location
            .file()
            .trim_start_matches(concat!(env!("CARGO_MANIFEST_DIR"), "/src/"))
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("crate", &"s2n-quic-dc")
            .field("file", &self.file())
            .field("line", &self.location.line())
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { kind, location } = self;
        let file = self.file();
        let line = location.line();
        write!(f, "[s2n-quic-dc::{file}:{line}]: {kind}")
    }
}

impl core::error::Error for Error {}

impl From<Kind> for Error {
    #[track_caller]
    #[inline]
    fn from(kind: Kind) -> Self {
        Self::new(kind)
    }
}

impl From<Error> for std::io::Error {
    #[inline]
    #[track_caller]
    fn from(error: Error) -> Self {
        use std::io::ErrorKind;
        let kind = match error.kind {
            Kind::PeerPskMissing => ErrorKind::NotFound,
        };
        Self::new(kind, error)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Kind {
    /// No path secret is cached for the peer.
    #[error("no path secret is cached for the peer")]
    PeerPskMissing,
}

impl Kind {
    #[inline]
    #[track_caller]
    pub fn err(self) -> Error {
        Error::new(self)
    }

    /// Recovers the connect [`Kind`] from an [`std::io::Error`], if it originated as one.
    #[inline]
    pub fn from_io(err: &std::io::Error) -> Option<&Self> {
        Error::from_io(err).map(Error::kind)
    }
}

