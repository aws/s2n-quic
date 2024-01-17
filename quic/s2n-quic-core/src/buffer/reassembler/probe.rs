// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

crate::probe::define!(
    extern "probe" {
        /// Emitted when a buffer is allocated for a particular offset
        #[link_name = s2n_quic_core__buffer__reassembler__alloc]
        pub fn alloc(offset: u64, capacity: usize);

        /// Emitted when a chunk is read from the beginning of the buffer
        #[link_name = s2n_quic_core__buffer__reassembler__pop]
        pub fn pop(offset: u64, len: usize);

        /// Emitted when a chunk of data is written at an offset
        #[link_name = s2n_quic_core__buffer__reassembler__write]
        pub fn write(offset: u64, len: usize);
    }
);
