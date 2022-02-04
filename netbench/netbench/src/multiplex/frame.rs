// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, Result};
use bytes::{Buf, BufMut, Bytes};
use core::mem::size_of;
use enum_primitive_derive::Primitive;
use num_traits::FromPrimitive;
use std::io;

#[derive(Clone, Copy, Debug, Primitive)]
#[repr(u8)]
enum Tag {
    StreamOpen = 0,
    StreamData = 1,
    StreamFinish = 2,
    MaxStreams = 3,
    MaxStreamData = 4,
    InitialMaxStreamData = 5,
}

#[derive(Clone, Debug)]
pub enum Frame {
    StreamOpen { id: u64, bidirectional: bool },
    StreamData { id: u64, owner: Owner, data: Bytes },
    StreamFinish { id: u64, owner: Owner },
    MaxStreams { up_to: u64 },
    MaxStreamData { id: u64, owner: Owner, up_to: u64 },
    InitialMaxStreamData { up_to: u64 },
}

impl Frame {
    pub fn write_header<B: BufMut>(&self, buf: &mut B) {
        match self {
            Self::StreamOpen { id, bidirectional } => {
                buf.put_u8(Tag::StreamOpen as _);
                buf.put_u64(*id);
                buf.put_u8(*bidirectional as u8);
            }
            Self::StreamData { id, owner, data } => {
                buf.put_u8(Tag::StreamData as _);
                buf.put_u64(*id);
                buf.put_u8(*owner as _);
                buf.put_u32(data.len() as _);
            }
            Self::StreamFinish { id, owner } => {
                buf.put_u8(Tag::StreamFinish as _);
                buf.put_u64(*id);
                buf.put_u8(*owner as _);
            }
            Self::MaxStreams { up_to } => {
                buf.put_u8(Tag::MaxStreams as _);
                buf.put_u64(*up_to);
            }
            Self::MaxStreamData { id, owner, up_to } => {
                buf.put_u8(Tag::MaxStreamData as _);
                buf.put_u64(*id);
                buf.put_u8(*owner as _);
                buf.put_u64(*up_to);
            }
            Self::InitialMaxStreamData { up_to } => {
                buf.put_u8(Tag::InitialMaxStreamData as _);
                buf.put_u64(*up_to);
            }
        }
    }

    pub fn body(self) -> Option<Bytes> {
        if let Self::StreamData { data, .. } = self {
            Some(data)
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
pub struct Decoder {
    tag: Option<Tag>,
    stream: Option<(u64, Owner, usize)>,
}

impl Decoder {
    pub fn decode<B: Buf>(&mut self, buf: &mut B) -> Result<Option<Frame>> {
        if let Some((id, owner, len)) = self.stream.take() {
            return self.decode_stream(buf, id, owner, len);
        }

        if let Some(tag) = self.tag.take() {
            return self.decode_frame(buf, tag);
        }

        if buf.remaining() == 0 {
            return Ok(None);
        }

        let tag = Tag::from_u8(buf.get_u8())
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid frame tag"))?;

        self.decode_frame(buf, tag)
    }

    fn decode_frame<B: Buf>(&mut self, buf: &mut B, tag: Tag) -> Result<Option<Frame>> {
        match tag {
            Tag::StreamOpen => {
                if buf.remaining() < (size_of::<u64>() + size_of::<u8>()) {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let id = buf.get_u64();
                let bidirectional = buf.get_u8() != 0;

                Ok(Some(Frame::StreamOpen { id, bidirectional }))
            }
            Tag::StreamData => {
                if buf.remaining() < (size_of::<u64>() + size_of::<u8>() + size_of::<u32>()) {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let id = buf.get_u64();
                let owner = Self::decode_owner(buf)?;
                let len = buf.get_u32();
                self.decode_stream(buf, id, owner, len as _)
            }
            Tag::StreamFinish => {
                if buf.remaining() < (size_of::<u64>() + size_of::<u8>()) {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let id = buf.get_u64();
                let owner = Self::decode_owner(buf)?;

                Ok(Some(Frame::StreamFinish { id, owner }))
            }
            Tag::MaxStreams => {
                if buf.remaining() < size_of::<u64>() {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let up_to = buf.get_u64();

                Ok(Some(Frame::MaxStreams { up_to }))
            }
            Tag::MaxStreamData => {
                if buf.remaining() < (size_of::<u64>() + size_of::<u8>() + size_of::<u64>()) {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let id = buf.get_u64();
                let owner = Self::decode_owner(buf)?;
                let up_to = buf.get_u64();

                Ok(Some(Frame::MaxStreamData { id, owner, up_to }))
            }
            Tag::InitialMaxStreamData => {
                if buf.remaining() < (size_of::<u64>()) {
                    self.tag = Some(tag);
                    return Ok(None);
                }

                let up_to = buf.get_u64();

                Ok(Some(Frame::InitialMaxStreamData { up_to }))
            }
        }
    }

    fn decode_owner<B: Buf>(buf: &mut B) -> Result<Owner> {
        let owner = buf.get_u8();
        Ok(match owner {
            0 => Owner::Local,
            1 => Owner::Remote,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid owner id").into()),
        })
    }

    fn decode_stream<B: Buf>(
        &mut self,
        buf: &mut B,
        id: u64,
        owner: Owner,
        len: usize,
    ) -> Result<Option<Frame>> {
        if len == 0 {
            return self.decode(buf);
        }

        let chunk_len = buf.chunk().len();

        if chunk_len == 0 {
            self.stream = Some((id, owner, len));
            return Ok(None);
        }

        Ok(if chunk_len >= len {
            let data = buf.copy_to_bytes(len);
            Some(Frame::StreamData { id, owner, data })
        } else {
            let data = buf.copy_to_bytes(chunk_len);
            self.stream = Some((id, owner, len - chunk_len));
            Some(Frame::StreamData { id, owner, data })
        })
    }
}
