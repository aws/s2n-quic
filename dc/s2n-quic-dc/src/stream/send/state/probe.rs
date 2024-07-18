// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{buffer, varint::VarInt};

#[derive(Clone, Copy, Debug)]
pub struct Probe {
    pub offset: VarInt,
    pub final_offset: Option<VarInt>,
}

impl buffer::reader::Storage for Probe {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        0
    }

    #[inline]
    fn read_chunk(
        &mut self,
        _watermark: usize,
    ) -> Result<buffer::reader::storage::Chunk<'_>, Self::Error> {
        Ok(Default::default())
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        _dest: &mut Dest,
    ) -> Result<buffer::reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: buffer::writer::Storage + ?Sized,
    {
        Ok(Default::default())
    }
}

impl buffer::Reader for Probe {
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.offset
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.final_offset
    }
}
