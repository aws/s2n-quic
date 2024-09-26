// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// A Recorder should arrange to emit the properties and counters on Drop into some output.
pub trait Recorder: 'static + Send + Sync {
    /// Registers a counter with the recorder instance
    fn increment_counter(&self, name: &str, amount: usize);

    /// Associates a key/value pair with the recorder instance
    fn set_value<V: core::fmt::Display>(&self, key: &str, value: V);
}
