// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Id(String);

impl Id {
    pub(crate) fn hasher() -> Hasher {
        Hasher::default()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
        let out = base64::encode_config(hash, base64::URL_SAFE_NO_PAD);
        // '_' is not allowed in DNS names
        let out = out.replace('_', "-");
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
