// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncWrite};

/// Simple testing connection with a single bidirectional stream
pub struct Connection {
    local: Arc<Mutex<Stream>>,
    peer: Arc<Mutex<Stream>>,
}

impl Connection {
    /// Creates a pair of connections with the specified buffer limit
    pub fn pair(max_buffer: usize) -> (Connection, Connection) {
        let client: Arc<Mutex<Stream>> = Arc::new(Mutex::new(Stream::new(max_buffer)));
        let server: Arc<Mutex<Stream>> = Arc::new(Mutex::new(Stream::new(max_buffer)));
        let client_conn = Connection {
            local: client.clone(),
            peer: server.clone(),
        };
        let server_conn = Connection {
            local: server,
            peer: client,
        };
        (client_conn, server_conn)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Ok(mut stream) = self.local.lock() {
            stream.close();
        }
        if let Ok(mut stream) = self.peer.lock() {
            stream.close();
        }
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut stream = self.local.lock().unwrap();
        Pin::new(&mut *stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut stream = self.peer.lock().unwrap();
        Pin::new(&mut *stream).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut stream = self.peer.lock().unwrap();
        Pin::new(&mut *stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut stream = self.peer.lock().unwrap();
        Pin::new(&mut *stream).poll_shutdown(cx)
    }
}

#[derive(Debug, Default)]
struct Stream {
    buffer: Vec<u8>,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    is_closed: bool,
    max_buffer: usize,
}

impl Stream {
    fn new(max_buffer: usize) -> Self {
        Self {
            max_buffer,
            ..Default::default()
        }
    }

    fn close(&mut self) {
        self.is_closed = true;
        self.wake_writer();
        self.wake_reader();
    }

    fn wake_reader(&mut self) {
        if let Some(waker) = self.read_waker.take() {
            waker.wake();
        }
    }

    fn wake_writer(&mut self) {
        if let Some(waker) = self.write_waker.take() {
            waker.wake();
        }
    }

    fn ensure_open(&self) -> io::Result<()> {
        if self.is_closed {
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "closed"))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.buffer.is_empty() {
            // do an empty write
            if self.is_closed {
                return Ok(()).into();
            }

            self.read_waker = Some(context.waker().clone());
            return Poll::Pending;
        }

        let len = self.buffer.len().min(buf.remaining());
        buf.put_slice(&self.buffer[..len]);
        self.buffer.drain(..len);

        // let the peer know it can write more
        self.wake_writer();

        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.ensure_open()?;

        let len = (self.max_buffer - self.buffer.len()).min(buf.len());

        if len == 0 {
            self.write_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        self.buffer.extend_from_slice(&buf[..len]);

        // let the peer know it can read more
        self.wake_reader();

        Poll::Ready(Ok(len))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.ensure_open()?;

        if self.buffer.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            self.write_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        if !self.buffer.is_empty() {
            self.write_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        self.is_closed = true;

        // let the peer know we're closed now
        self.wake_reader();

        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn read_write_shutdown() -> io::Result<()> {
        let (mut client, mut server) = Connection::pair(100);

        tokio::spawn(async move {
            client.write_all(b"hello!").await?;
            client.shutdown().await?;
            io::Result::Ok(())
        });

        let mut buf = vec![];
        server.read_to_end(&mut buf).await?;

        assert_eq!(buf, b"hello!");

        Ok(())
    }

    #[tokio::test]
    async fn read_write_drop() -> io::Result<()> {
        let (mut client, mut server) = Connection::pair(100);

        tokio::spawn(async move {
            client.write_all(b"hello!").await?;
            // let the drop handle the connection shutdown
            io::Result::Ok(())
        });

        let mut buf = vec![];
        server.read_to_end(&mut buf).await?;

        assert_eq!(buf, b"hello!");

        Ok(())
    }
}
