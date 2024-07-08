// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path::secret::{Map, Opener, Sealer};
use core::{fmt, sync::atomic::Ordering};
use crossbeam_epoch::{pin, Atomic};

// TODO support key updates
pub struct Crypto {
    sealer: Atomic<Sealer>,
    opener: Atomic<Opener>,
    map: Map,
}

impl fmt::Debug for Crypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Crypto")
            .field("sealer", &self.sealer)
            .field("opener", &self.opener)
            .finish()
    }
}

impl Crypto {
    #[inline]
    pub fn new(sealer: Sealer, opener: Opener, map: &Map) -> Self {
        let sealer = Atomic::new(sealer);
        let opener = Atomic::new(opener);
        Self {
            sealer,
            opener,
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
    pub fn seal_with<R>(&self, seal: impl FnOnce(&Sealer) -> R) -> R {
        let pin = pin();
        let sealer = self.seal_pin(&pin);
        seal(sealer)
    }

    #[inline]
    fn seal_pin<'a>(&self, pin: &'a crossbeam_epoch::Guard) -> &'a Sealer {
        let sealer = self.sealer.load(Ordering::Acquire, pin);
        unsafe { sealer.deref() }
    }

    #[inline]
    pub fn open_with<R>(&self, open: impl FnOnce(&Opener) -> R) -> R {
        let pin = pin();
        let opener = self.open_pin(&pin);
        open(opener)
    }

    #[inline]
    fn open_pin<'a>(&self, pin: &'a crossbeam_epoch::Guard) -> &'a Opener {
        let opener = self.opener.load(Ordering::Acquire, pin);
        unsafe { opener.deref() }
    }
}

impl Drop for Crypto {
    #[inline]
    fn drop(&mut self) {
        use crossbeam_epoch::Shared;
        let pin = pin();
        let sealer = self.sealer.swap(Shared::null(), Ordering::AcqRel, &pin);
        let opener = self.opener.swap(Shared::null(), Ordering::AcqRel, &pin);

        // no need to drop either one
        if sealer.is_null() && opener.is_null() {
            return;
        }

        unsafe {
            pin.defer_unchecked(move || {
                drop(sealer.try_into_owned());
                drop(opener.try_into_owned());
            })
        }
    }
}
