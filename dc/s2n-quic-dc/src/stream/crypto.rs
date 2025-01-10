// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::awslc::{open, seal},
    event,
    path::secret::{open::Application as Opener, seal::Application as Sealer, Map},
    stream::shared,
};
use core::fmt;
use s2n_quic_core::time::Clock;
use std::sync::Mutex;

pub struct Crypto {
    app_sealer: Mutex<Sealer>,
    app_opener: Mutex<Opener>,
    control_sealer: Option<seal::control::Stream>,
    control_opener: Option<open::control::Stream>,
    map: Map,
}

impl fmt::Debug for Crypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Crypto")
            .field("sealer", &self.app_sealer)
            .field("opener", &self.app_opener)
            .finish()
    }
}

impl Crypto {
    #[inline]
    pub fn new(
        app_sealer: Sealer,
        app_opener: Opener,
        control: Option<(seal::control::Stream, open::control::Stream)>,
        map: &Map,
    ) -> Self {
        let app_sealer = Mutex::new(app_sealer);
        let app_opener = Mutex::new(app_opener);
        let (control_sealer, control_opener) = if let Some((s, o)) = control {
            (Some(s), Some(o))
        } else {
            (None, None)
        };
        Self {
            app_sealer,
            app_opener,
            control_sealer,
            control_opener,
            map: map.clone(),
        }
    }

    #[inline(always)]
    pub fn tag_len(&self) -> usize {
        16
    }

    #[inline]
    pub fn map(&self) -> &Map {
        &self.map
    }

    #[inline]
    pub fn seal_with<R>(
        &self,
        seal: impl FnOnce(&Sealer) -> R,
        update: impl FnOnce(&mut Sealer),
    ) -> R {
        let lock = &self.app_sealer;
        let mut guard = lock.lock().unwrap();
        let result = seal(&guard);

        // update the keys if needed
        if guard.needs_update() {
            update(&mut guard);
        }

        result
    }

    #[inline]
    pub fn open_with<C: Clock + ?Sized, Sub: event::Subscriber, R>(
        &self,
        open: impl FnOnce(&Opener) -> R,
        clock: &C,
        subscriber: &shared::Subscriber<Sub>,
    ) -> R {
        let lock = &self.app_opener;
        let mut guard = lock.lock().unwrap();
        let result = open(&guard);

        // update the keys if needed
        if guard.needs_update() {
            guard.update(clock, subscriber);
        }

        result
    }

    #[inline]
    pub fn control_sealer(&self) -> Option<&seal::control::Stream> {
        self.control_sealer.as_ref()
    }

    #[inline]
    pub fn control_opener(&self) -> Option<&open::control::Stream> {
        self.control_opener.as_ref()
    }
}
