use crate::rpc;
use bytes::BytesMut;
use s2n_quic_core::buffer::{
    reader::{self, storage::Infallible as _, Storage as _},
    writer::{self, Storage as _},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct Borrowed<'a, S>(&'a mut S);

impl<'a, S> Borrowed<'a, S> {
    #[inline]
    pub fn new(stream: &'a mut S) -> Self {
        Self(stream)
    }
}

impl<'a, S> AsyncRead for Borrowed<'a, S>
where
    S: AsyncRead + Send + Unpin,
{
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<'a, S> AsyncWrite for Borrowed<'a, S>
where
    S: AsyncWrite + Send + Unpin,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }

    #[inline]
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.get_mut().0).poll_write_vectored(cx, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum StreamLen {
    #[default]
    Unknown,
    Known(usize),
    Chunked,
}

pub struct Reader<S> {
    total_len: StreamLen,
    read: usize,
    buffer: BytesMut,
    stream: S,
}

impl<S> Reader<S>
where
    S: AsyncRead + Unpin,
{
    #[inline]
    pub fn new(stream: S) -> Self {
        Self {
            total_len: StreamLen::default(),
            read: 0,
            buffer: BytesMut::new(),
            stream,
        }
    }

    #[inline]
    async fn stream_len(&mut self) -> io::Result<Option<usize>> {
        loop {
            match self.total_len {
                StreamLen::Known(len) => return Ok(Some(len)),
                StreamLen::Chunked => return Ok(None),
                _ => {}
            }

            let len = self.stream.read_u64().await?;

            if len == u64::MAX {
                self.total_len = StreamLen::Chunked;
                continue;
            }

            let len = len
                .try_into()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

            self.total_len = StreamLen::Known(len);
        }
    }
}

impl<S> rpc::client::Response for Reader<S>
where
    S: AsyncRead + Send + Unpin,
{
    #[inline]
    fn read_chunk<Payload>(
        &mut self,
        payload: &mut Payload,
    ) -> impl Future<Output = io::Result<usize>>
    where
        Payload: writer::Storage,
    {
        async move {
            let mut stream_len = self.stream_len().await?;

            loop {
                // check if the payload has any remaining capacity
                if !payload.has_remaining_capacity() {
                    return Ok(0);
                }

                // drain what we've buffered so far
                if !self.buffer.is_empty() {
                    let mut buffer = self.buffer.track_read();
                    buffer.infallible_copy_into(payload);
                    return Ok(buffer.consumed_len());
                }

                if let Some(total_len) = stream_len {
                    // check if we've already hit the end of the stream
                    if total_len == self.read {
                        return Ok(0);
                    }
                }

                /// Don't allocate more than 4Gb at a time
                const MAX_ALLOCATION: usize = u32::MAX as _;

                let (min_remaining, allocation_target) = if let Some(total_len) = stream_len {
                    let remaining = total_len - self.read;
                    if remaining > MAX_ALLOCATION {
                        (1024, MAX_ALLOCATION)
                    } else {
                        (1, remaining)
                    }
                } else {
                    (1024, u16::MAX as _)
                };

                if self.buffer.remaining_capacity() < min_remaining {
                    debug_assert!(self.buffer.is_empty());
                    self.buffer = BytesMut::with_capacity(allocation_target);
                }

                // TODO write directly into the provided `payload`, if possible.
                //      - s2n-quic buffer traits need a way to get an uninit slice and only write partially
                let read_len = Pin::new(&mut self.stream)
                    .read_buf(&mut self.buffer)
                    .await?;

                // check if the stream was terminated
                if read_len == 0 {
                    self.total_len = StreamLen::Known(self.read);
                    stream_len = Some(self.read);
                } else {
                    self.read += read_len;
                }
            }
        }
    }
}

pub struct Writer<S> {
    total: usize,
    chunked: bool,
    stream: S,
}

impl<S> Writer<S>
where
    S: AsyncRead + Unpin,
{
    #[inline]
    pub fn new(stream: S) -> Self {
        Self {
            total: 0,
            chunked: false,
            stream,
        }
    }
}

impl<S> rpc::server::Response for Writer<S>
where
    S: AsyncWrite + Send + Unpin,
{
    #[inline]
    fn write_chunk<Payload>(
        &mut self,
        payload: &mut Payload,
    ) -> impl Future<Output = io::Result<usize>>
    where
        Payload: reader::storage::Infallible,
    {
        async move {
            // if we haven't written any data yet, set it to the max payload size to indicate
            // it's being written in chunks
            // TODO chain this on the payload to reduce syscalls
            if !core::mem::replace(&mut self.chunked, true) {
                self.stream.write_all(&u64::MAX.to_be_bytes()).await?;
            }

            let chunk = payload.infallible_read_chunk(usize::MAX);
            if chunk.is_empty() {
                return Ok(0);
            }
            self.stream.write_all(&chunk).await?;
            self.total += chunk.len();
            return Ok(chunk.len());
        }
    }

    #[inline]
    fn write_to_end<Payload>(
        mut self,
        mut payload: Payload,
    ) -> impl Future<Output = std::io::Result<usize>>
    where
        Payload: reader::storage::Infallible,
    {
        async move {
            if !self.chunked {
                // Write a `len` prefix so we don't need to rely on TCP shutdown. It's usually faster
                // to use `close` instead.
                // TODO chain this on the payload
                let len = payload.buffered_len() as u64;
                self.stream.write_all(&len.to_be_bytes()).await?;
            }

            // TODO Add a method in s2n-quic's buffer impl to make this more efficient.
            //      Due to lifetimes, it currently needs to process a chunk at a time.
            loop {
                let chunk = payload.infallible_read_chunk(usize::MAX);
                if chunk.is_empty() {
                    break;
                }
                self.stream.write_all(&chunk).await?;
                self.total += chunk.len();
            }

            Ok(self.total)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{client::Response as _, server::Response as _};
    use tokio::io::duplex;

    #[tokio::test]
    async fn reader_writer() {
        let (reader, writer) = duplex(1024);
        let reader = Reader::new(reader);
        let writer = Writer::new(writer);

        let request = &b"hello world!"[..];
        let written = writer.write_to_end(request).await.unwrap();
        assert_eq!(written, request.len());

        let mut received: Vec<BytesMut> = vec![];
        reader.read_to_end(&mut received).await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], request);
    }
}
