// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt::Write;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Hash)]
pub struct Id(String);

impl Id {
    pub(crate) fn hasher() -> Hasher {
        Hasher::default()
    }
}

#[derive(Debug, Default)]
pub struct Hasher {
    hash: sha2::Sha256,
}

impl Hasher {
    pub fn finish(self) -> Id {
        use sha2::Digest;
        let hash = self.hash.finalize();
        let mut out = String::new();
        for byte in hash.iter() {
            write!(out, "{:02x}", byte).unwrap();
        }
        Id(out)
    }
}

impl core::hash::Hasher for Hasher {
    fn write(&mut self, bytes: &[u8]) {
        use sha2::Digest;
        self.hash.update(bytes)
    }

    fn finish(&self) -> u64 {
        unimplemented!()
    }
}
