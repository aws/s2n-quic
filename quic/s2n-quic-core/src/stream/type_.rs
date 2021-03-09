// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// The Stream Type defines whether data can be transmitted in both directions
/// or only in a single direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    /// Data can be transmitted on the Stream in both directions
    Bidirectional,
    /// Data can be transmitted on the Stream only in a single direction
    Unidirectional,
}

impl StreamType {
    pub fn is_bidirectional(self) -> bool {
        self == StreamType::Bidirectional
    }

    pub fn is_unidirectional(self) -> bool {
        self == StreamType::Unidirectional
    }
}
