// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, Result};
use bytes::Buf;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use std::collections::{hash_map::Entry, HashMap, HashSet, VecDeque};
use tokio::io::{AsyncRead, AsyncWrite};

mod buffer;
mod frame;
mod stream;

use buffer::{ReadBuffer, WriteBuffer};
use frame::Frame;
use stream::{ReceiveStream, SendStream, Stream};

const DEFAULT_STREAM_WINDOW: u64 = 256000;
const DEFAULT_MAX_STREAMS: u64 = 100;

#[derive(Clone, Debug)]
pub struct Config {
    pub stream_window: u64,
    pub max_streams: u64,
    pub max_stream_data_thresh: u64,
    pub max_stream_frame_len: u32,
    pub max_write_queue_len: usize,
    pub peer_max_streams: u64,
    pub peer_max_stream_data: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stream_window: DEFAULT_STREAM_WINDOW,
            max_streams: DEFAULT_MAX_STREAMS,
            max_stream_data_thresh: DEFAULT_STREAM_WINDOW / 2,
            max_stream_frame_len: (1 << 15),
            max_write_queue_len: 250,
            peer_max_streams: DEFAULT_MAX_STREAMS,
            peer_max_stream_data: DEFAULT_STREAM_WINDOW,
        }
    }
}

#[derive(Debug)]
pub struct Connection<T: AsyncRead + AsyncWrite> {
    inner: Pin<Box<T>>,
    rx_open: bool,
    tx_open: bool,
    frame: Option<frame::Frame>,
    read_buf: ReadBuffer,
    decoder: frame::Decoder,
    write_buf: WriteBuffer,
    stream_controller: stream::Controller,
    streams: [HashMap<u64, Stream>; 2],
    max_stream_data: HashSet<(Owner, u64)>,
    pending_accept: VecDeque<u64>,
    config: Config,
}

impl<T: AsyncRead + AsyncWrite> Connection<T> {
    pub fn new(inner: Pin<Box<T>>, config: Config) -> Self {
        let mut write_buf = WriteBuffer::default();

        if config.stream_window != DEFAULT_STREAM_WINDOW {
            write_buf.push(Frame::InitialMaxStreamData {
                up_to: config.stream_window,
            });
        }

        if config.max_streams != DEFAULT_MAX_STREAMS {
            write_buf.push(Frame::MaxStreams {
                up_to: config.max_streams,
            });
        }

        Self {
            inner,
            rx_open: true,
            tx_open: true,
            frame: None,
            read_buf: Default::default(),
            decoder: Default::default(),
            write_buf,
            stream_controller: stream::Controller::new(config.max_streams, config.peer_max_streams),
            streams: [Default::default(), Default::default()],
            max_stream_data: Default::default(),
            pending_accept: Default::default(),
            config,
        }
    }

    fn flush_write_buffer(&mut self, cx: &mut Context) -> Result<()> {
        if !self.tx_open {
            return if self.write_buf.is_empty() {
                Ok(())
            } else {
                Err("stream was closed with pending data".into())
            };
        }

        if self.write_buf.is_empty() {
            return Ok(());
        }

        if self.inner.as_ref().is_write_vectored() {
            let chunks = self.write_buf.chunks();

            return match self.inner.as_mut().poll_write_vectored(cx, &chunks) {
                Poll::Ready(result) => {
                    let len = result?;
                    self.write_buf.advance(len);
                    self.write_buf.notify(cx);
                    Ok(())
                }
                Poll::Pending => Ok(()),
            };
        }

        while let Some(mut chunk) = self.write_buf.pop_front() {
            match self.inner.as_mut().poll_write(cx, &chunk) {
                Poll::Ready(result) => {
                    let len = result?;
                    chunk.advance(len);

                    if !chunk.is_empty() {
                        self.write_buf.push_front(chunk);
                    }

                    if len == 0 {
                        return if self.write_buf.is_empty() {
                            self.tx_open = false;
                            Ok(())
                        } else {
                            Err("stream was closed with pending data".into())
                        };
                    }
                }
                Poll::Pending => {
                    self.write_buf.push_front(chunk);
                    break;
                }
            }
        }

        self.write_buf.notify(cx);

        Ok(())
    }

    fn flush_read_buffer(&mut self, cx: &mut Context) -> Result<()> {
        loop {
            if self.frame.is_none() {
                if let Some(frame) = self.decoder.decode(&mut self.read_buf)? {
                    self.frame = Some(frame);
                }
            }

            match self.dispatch_frame(cx) {
                Poll::Ready(result) => result?,
                Poll::Pending => return Ok(()),
            }
        }
    }

    fn fill_read_buffer(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        // don't fill the buffer if we have a pending frame
        if self.frame.is_some() {
            return Poll::Pending;
        }

        // the socket is closed
        if !self.rx_open {
            return Ok(()).into();
        }

        let rx_open = &mut self.rx_open;
        let inner = self.inner.as_mut();

        ready!(self.read_buf.read(|buf| {
            ready!(inner.poll_read(cx, buf))?;

            // the socket returned a 0 write
            if buf.filled().is_empty() {
                *rx_open = false;
            }

            Ok(()).into()
        }))?;

        Ok(()).into()
    }

    fn dispatch_frame(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        use frame::Frame::*;

        match self.frame.take() {
            Some(StreamOpen { id, bidirectional }) => {
                // TODO make sure the peer hasn't opened too many
                let mut stream = Stream {
                    rx: Some(ReceiveStream::new(self.config.stream_window)),
                    tx: None,
                };
                if bidirectional {
                    stream.tx = Some(SendStream::new(self.config.peer_max_stream_data));
                };
                self.streams[Owner::Remote].insert(id, stream);
                self.pending_accept.push_back(id);
                cx.waker().wake_by_ref();
            }
            Some(StreamData {
                id,
                owner,
                mut data,
            }) => {
                if let Some(rx) = self.streams[owner]
                    .get_mut(&id)
                    .and_then(|stream| stream.rx.as_mut())
                {
                    let len = data.len() as u64;
                    let len = rx.buffer(len)?;
                    data.advance(len as _);

                    if !data.is_empty() {
                        self.frame = Some(StreamData { id, owner, data });
                        return Poll::Pending;
                    }
                }
            }
            Some(MaxStreams { up_to }) => {
                self.stream_controller.max_streams(up_to);
            }
            Some(MaxStreamData { id, owner, up_to }) => {
                if let Some(tx) = self.streams[owner]
                    .get_mut(&id)
                    .and_then(|stream| stream.tx.as_mut())
                {
                    tx.max_data(up_to);
                }
            }
            Some(StreamFinish { id, owner }) => {
                if let Some(rx) = self.streams[owner]
                    .get_mut(&id)
                    .ok_or("invalid stream")?
                    .rx
                    .as_mut()
                {
                    rx.finish();
                }
            }
            Some(InitialMaxStreamData { up_to }) => {
                self.config.peer_max_stream_data = up_to;
            }
            None => return Poll::Pending,
        }

        Ok(()).into()
    }
}

impl<T: AsyncRead + AsyncWrite> super::Connection for Connection<T> {
    fn poll_open_bidirectional_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        ready!(self.stream_controller.open(cx));

        self.write_buf.push(Frame::StreamOpen {
            id,
            bidirectional: true,
        });

        let stream = Stream {
            rx: Some(ReceiveStream::new(self.config.stream_window)),
            tx: Some(SendStream::new(self.config.peer_max_stream_data)),
        };
        self.streams[Owner::Local].insert(id, stream);

        Ok(()).into()
    }

    fn poll_open_send_stream(&mut self, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        ready!(self.stream_controller.open(cx));

        self.write_buf.push(Frame::StreamOpen {
            id,
            bidirectional: false,
        });

        let stream = Stream {
            rx: None,
            tx: Some(SendStream::new(self.config.peer_max_stream_data)),
        };
        self.streams[Owner::Local].insert(id, stream);

        Ok(()).into()
    }

    fn poll_accept_stream(&mut self, _cx: &mut Context) -> Poll<Result<Option<u64>>> {
        if !self.rx_open {
            return Ok(None).into();
        }

        if let Some(id) = self.pending_accept.pop_front() {
            Ok(Some(id)).into()
        } else {
            Poll::Pending
        }
    }

    fn poll_send(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>> {
        if !self.write_buf.request_push(self.config.max_write_queue_len) {
            return Poll::Pending;
        }

        let stream = self.streams[owner]
            .get_mut(&id)
            .ok_or("missing stream")?
            .tx
            .as_mut()
            .ok_or("missing tx stream")?;

        let allowed_bytes = bytes.min(self.config.max_stream_frame_len as _);

        if let Some(data) = stream.send(allowed_bytes, cx) {
            let len = data.len() as u64;
            self.write_buf.push(frame::Frame::StreamData {
                id,
                owner: !owner,
                data,
            });
            Ok(len).into()
        } else {
            Poll::Pending
        }
    }

    fn poll_receive(
        &mut self,
        owner: Owner,
        id: u64,
        bytes: u64,
        cx: &mut Context,
    ) -> Poll<Result<u64>> {
        let stream = self.streams[owner]
            .get_mut(&id)
            .ok_or("missing stream")?
            .rx
            .as_mut()
            .ok_or("missing rx stream")?;

        let len = ready!(stream.receive(bytes, cx))?;

        if stream.receive_window() < self.config.stream_window / 2 {
            self.max_stream_data.insert((owner, id));
        }

        Ok(len).into()
    }

    fn poll_send_finish(&mut self, owner: Owner, id: u64, _cx: &mut Context) -> Poll<Result<()>> {
        if !self.write_buf.request_push(self.config.max_write_queue_len) {
            return Poll::Pending;
        }

        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();

            if stream.tx.take().is_some() {
                self.write_buf
                    .push(frame::Frame::StreamFinish { id, owner: !owner });
            }

            if stream.rx.is_none() {
                entry.remove();
                self.stream_controller.close();
            }
        }

        Ok(()).into()
    }

    fn poll_receive_finish(&mut self, owner: Owner, id: u64, cx: &mut Context) -> Poll<Result<()>> {
        if let Entry::Occupied(mut entry) = self.streams[owner].entry(id) {
            let stream = entry.get_mut();

            if let Some(rx) = stream.rx.as_mut() {
                ready!(rx.poll_finish(cx));
            }
            stream.rx = None;

            if stream.tx.is_none() {
                entry.remove();
                self.stream_controller.close();
            }
        }

        Ok(()).into()
    }

    fn poll_progress(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        loop {
            if let Some(up_to) = self.stream_controller.transmit() {
                self.write_buf.push_priority(Frame::MaxStreams { up_to });
            }

            for (owner, id) in self.max_stream_data.drain() {
                let stream = self.streams[owner]
                    .get_mut(&id)
                    .unwrap()
                    .rx
                    .as_mut()
                    .unwrap();
                let up_to = stream.credit(self.config.stream_window);
                self.write_buf.push_priority(Frame::MaxStreamData {
                    id,
                    owner: !owner,
                    up_to,
                });
            }

            self.flush_write_buffer(cx)?;
            self.flush_read_buffer(cx)?;
            ready!(self.fill_read_buffer(cx))?;

            // the connection is done
            if !self.rx_open && self.frame.is_none() {
                return Ok(()).into();
            }
        }
    }

    fn poll_finish(&mut self, cx: &mut Context) -> Poll<Result<()>> {
        if self.tx_open {
            self.flush_write_buffer(cx)?;

            // wait to shutdown the socket until we have written everything
            if !self.write_buf.is_empty() {
                return Poll::Pending;
            }

            // notify the peer we're not writing anything anymore
            ready!(self.inner.as_mut().poll_shutdown(cx))?;
            self.tx_open = false;
        }

        loop {
            // work to read all of the remaining data
            self.flush_read_buffer(cx)?;
            ready!(self.fill_read_buffer(cx))?;

            // the connection is done
            if !self.rx_open && self.frame.is_none() {
                return Ok(()).into();
            }
        }
    }
}

#[derive(Debug, Default)]
struct Blocked(Option<core::task::Waker>);

impl Blocked {
    pub fn block(&mut self, cx: &mut Context) {
        self.0 = Some(cx.waker().clone());
    }

    pub fn unblock(&mut self) {
        if let Some(waker) = self.0.take() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scenario::Scenario, testing, timer, trace::MemoryLogger, units::*, Driver};
    use futures_test::task::new_count_waker;
    use insta::assert_display_snapshot;
    use std::collections::HashSet;

    fn test(config: Config, scenario: &Scenario) -> (MemoryLogger, MemoryLogger) {
        let traces = &scenario.traces;

        let (client, server) = testing::Connection::pair(10000);

        let mut client = {
            let scenario = &scenario.clients[0].connections[0];
            let conn = Box::pin(client);
            let conn = super::Connection::new(conn, config.clone());
            Driver::new(scenario, conn)
        };
        let mut client_trace = MemoryLogger::new(0, traces.clone());
        let mut client_checkpoints = HashSet::new();
        let mut client_timer = timer::Testing::default();

        let mut server = {
            let scenario = &scenario.servers[0].connections[0];
            let conn = Box::pin(server);
            let conn = super::Connection::new(conn, config);
            Driver::new(scenario, conn)
        };
        let mut server_trace = MemoryLogger::new(1, traces.clone());
        let mut server_checkpoints = HashSet::new();
        let mut server_timer = timer::Testing::default();

        let (waker, count) = new_count_waker();
        let mut prev_count = 0;
        let mut cx = core::task::Context::from_waker(&waker);

        loop {
            let c = client.poll_with_timer(
                &mut client_trace,
                &mut client_checkpoints,
                &mut client_timer,
                &mut cx,
            );
            let s = server.poll_with_timer(
                &mut server_trace,
                &mut server_checkpoints,
                &mut server_timer,
                &mut cx,
            );

            match (c, s) {
                (Poll::Ready(Ok(())), Poll::Ready(Ok(()))) => break,
                (Poll::Ready(Err(e)), _) | (_, Poll::Ready(Err(e))) => panic!("{}", e),
                _ => {
                    let current_count = count.get();
                    if current_count > prev_count {
                        prev_count = current_count;
                        continue;
                    }

                    if client_timer.advance_pair(&mut server_timer).is_none() {
                        eprintln!("the timer did not advance!");
                        eprintln!("server trace:");
                        eprintln!("{}", server_trace.as_str().unwrap());
                        eprintln!("{:#?}", server);
                        eprintln!("client trace:");
                        eprintln!("{}", client_trace.as_str().unwrap());
                        eprintln!("{:#?}", client);
                        panic!("test is deadlocked");
                    }
                }
            }
        }

        (client_trace, server_trace)
    }

    macro_rules! test {
        ($name:ident, $config:expr, $builder:expr) => {
            #[test]
            fn $name() -> crate::Result<()> {
                let scenario = Scenario::build(|scenario| {
                    let server = scenario.create_server();

                    scenario.create_client(|client| {
                        client.connect_to(server, $builder);
                    });
                });

                let (client_trace, server_trace) = test($config, &scenario);

                assert_display_snapshot!(
                    concat!(stringify!($name), "__client"),
                    client_trace.as_str().unwrap()
                );
                assert_display_snapshot!(
                    concat!(stringify!($name), "__server"),
                    server_trace.as_str().unwrap()
                );

                Ok(())
            }
        };
    }

    test!(single_stream, Config::default(), |conn| {
        conn.open_send_stream(
            |local| {
                local.send(1.kilobytes());
            },
            |remote| {
                remote.receive(1.kilobytes());
            },
        );
    });

    test!(single_slow_send_stream, Config::default(), |conn| {
        conn.open_send_stream(
            |local| {
                local.set_send_rate(100.bytes() / 50.millis());
                local.send(1.kilobytes());
            },
            |remote| {
                remote.receive(1.kilobytes());
            },
        );
    });

    test!(single_slow_recv_stream, Config::default(), |conn| {
        conn.open_send_stream(
            |local| {
                local.send(1.kilobytes());
            },
            |remote| {
                remote.set_receive_rate(100.bytes() / 50.millis());
                remote.receive(1.kilobytes());
            },
        );
    });

    test!(
        low_stream_window,
        Config {
            stream_window: 50,
            ..Config::default()
        },
        |conn| {
            conn.open_send_stream(
                |local| {
                    local.set_send_rate(100.bytes() / 50.millis());
                    local.send(1.kilobytes());
                },
                |remote| {
                    remote.receive(1.kilobytes());
                },
            );
        }
    );

    test!(
        low_max_streams,
        Config {
            max_streams: 2,
            peer_max_streams: 2,
            ..Config::default()
        },
        |conn| {
            conn.scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|conn| {
                        conn.open_send_stream(
                            |local| {
                                local.set_send_rate(500.bytes() / 50.millis());
                                local.send(1.kilobytes());
                            },
                            |remote| {
                                remote.receive(1.kilobytes());
                            },
                        );
                    });
                }
            });
        }
    );

    test!(multiple_streams, Config::default(), |conn| {
        conn.scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|conn| {
                    conn.open_send_stream(
                        |local| {
                            local.set_send_rate(100.bytes() / 50.millis());
                            local.send(1.kilobytes());
                        },
                        |remote| {
                            remote.receive(1.kilobytes());
                        },
                    );
                });
            }
        });
    });

    test!(checkpoint, Config::default(), |conn| {
        let (park, unpark) = conn.checkpoint();
        conn.scope(|scope| {
            scope.spawn(|conn| {
                conn.open_send_stream(
                    |local| {
                        local.set_send_rate(100.bytes() / 50.millis());
                        local.send(1.kilobytes());
                        local.unpark(unpark);
                        local.send(1.kilobytes());
                    },
                    |remote| {
                        remote.receive(2.kilobytes());
                    },
                );
            });
            scope.spawn(|conn| {
                conn.open_send_stream(
                    |local| {
                        local.park(park);
                        local.set_send_rate(100.bytes() / 50.millis());
                        local.send(1.kilobytes());
                    },
                    |remote| {
                        remote.receive(1.kilobytes());
                    },
                );
            });
        });
    });
}
