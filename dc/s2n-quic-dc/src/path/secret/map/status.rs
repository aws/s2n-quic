// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Entry, Map};
use crate::crypto;
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

// Retired is 0 if not yet retired. Otherwise it stores the background cleaner epoch at which it
// retired; that epoch increments roughly once per minute.
#[derive(Default)]
pub struct IsRetired(AtomicU64);

impl fmt::Debug for IsRetired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IsRetired")
            .field(&self.is_retired())
            .finish()
    }
}

impl IsRetired {
    pub fn retire(&self, at_epoch: u64) {
        self.0.store(at_epoch, Ordering::Relaxed);
    }

    pub fn retired_at(&self) -> Option<u64> {
        Some(self.0.load(Ordering::Relaxed)).filter(|v| *v > 0)
    }

    pub fn is_retired(&self) -> bool {
        self.retired_at().is_some()
    }
}

pub struct Dedup {
    cell: once_cell::sync::OnceCell<crypto::open::Result>,
    init: core::cell::Cell<Option<DedupInit>>,
}

struct DedupInit {
    entry: Arc<Entry>,
    key_id: VarInt,
    queue_id: Option<VarInt>,
    map: Map,
}

/// SAFETY: `init` cell is synchronized by `OnceCell`
unsafe impl Sync for Dedup {}

impl Dedup {
    #[inline]
    pub(super) fn new(
        entry: Arc<Entry>,
        key_id: VarInt,
        queue_id: Option<VarInt>,
        map: Map,
    ) -> Self {
        // TODO potentially record a timestamp of when this was created to try and detect long
        // delays of processing the first packet.
        Self {
            cell: Default::default(),
            init: core::cell::Cell::new(Some(DedupInit {
                entry,
                key_id,
                queue_id,
                map,
            })),
        }
    }

    #[inline]
    pub(crate) fn disabled() -> Self {
        Self {
            cell: once_cell::sync::OnceCell::with_value(Ok(())),
            init: core::cell::Cell::new(None),
        }
    }

    #[inline]
    pub(crate) fn disable(&self) {
        // TODO
    }

    #[inline]
    pub fn check(&self) -> crypto::open::Result {
        *self.cell.get_or_init(|| {
            match self.init.take() {
                Some(DedupInit {
                    entry,
                    key_id,
                    queue_id,
                    map,
                }) => map.store.check_dedup(&entry, key_id, queue_id),
                None => {
                    // Dedup has been poisoned! TODO log this
                    Err(crypto::open::Error::ReplayPotentiallyDetected { gap: None })
                }
            }
        })
    }
}

impl fmt::Debug for Dedup {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Dedup").field("cell", &self.cell).finish()
    }
}
