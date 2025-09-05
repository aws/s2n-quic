// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::testing;
use bolero::TypeGenerator;
use core::time::Duration;
use s2n_quic_core::stream::testing::Data;
use std::{io, panic::AssertUnwindSafe, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    time::timeout,
};
use tracing::{info_span, Instrument};

#[derive(Clone, Debug, TypeGenerator)]
pub struct PairOp {
    pub server: Vec<StreamOp>,
    pub client: Vec<StreamOp>,
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
pub enum StreamOp {
    Reader(ReaderOp),
    Writer(WriterOp),
    Sleep(#[generator(0..=100)] u8),
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
pub enum ReaderOp {
    Read(u16),
    Drop { panic: bool },
}

impl ReaderOp {
    async fn apply<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
        data: &mut Data,
        buffer: &mut [u8],
    ) -> io::Result<()> {
        match self {
            Self::Read(amount) => {
                let mut remaining = *amount as usize;
                loop {
                    let buffer_len = buffer.len().min(remaining);
                    let buffer = &mut buffer[..buffer_len];

                    let len = reader.read(buffer).await?;

                    // EOF
                    if len == 0 {
                        break;
                    }

                    data.receive(&[&buffer[..len]]);

                    remaining -= len;

                    if remaining == 0 {
                        break;
                    }
                }
                Ok(())
            }
            Self::Drop { .. } => {
                // no-op
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
pub enum WriterOp {
    Write(u16),
    Shutdown,
    Drop { panic: bool },
}

impl WriterOp {
    async fn apply<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        data: &mut Data,
    ) -> io::Result<()> {
        match self {
            Self::Write(amount) => {
                let mut chunks = [Default::default(), Default::default()];
                let chunk_len = data.send(*amount as usize, &mut chunks).unwrap();
                for chunk in &chunks[..chunk_len] {
                    writer.write_all(chunk).await?;
                }
                Ok(())
            }
            Self::Shutdown => writer.shutdown().await,
            Self::Drop { .. } => {
                // no-op
                Ok(())
            }
        }
    }
}

fn drop_while_panicking<T: 'static + Send>(value: T) {
    let value = AssertUnwindSafe(value);

    let _ = std::panic::catch_unwind(move || {
        let _value = value;
        // use resume_unwind instead of `panic` to avoid invoking the panic hook
        std::panic::resume_unwind(Box::new(()));
    });
}

pub struct Split<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    reader: Option<R>,
    reader_data: Data,
    reader_buffer: Vec<u8>,
    writer: Option<W>,
    writer_data: Data,
}

impl<R, W> Split<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: Some(reader),
            reader_data: Data::new(u64::MAX),
            reader_buffer: vec![0u8; u16::MAX as _],
            writer: Some(writer),
            writer_data: Data::new(u64::MAX),
        }
    }

    pub async fn apply_all(&mut self, ops: &[StreamOp]) -> Result<outcome::Stats, outcome::Error> {
        for (op_index, op) in ops.iter().enumerate() {
            self.apply(op).await.map_err(|err| outcome::Error::Io {
                op_index,
                kind: err.kind(),
            })?;
        }
        let recv = self.reader_data.offset();
        let send = self.writer_data.offset();
        Ok(outcome::Stats { recv, send })
    }

    pub async fn apply(&mut self, op: &StreamOp) -> io::Result<()> {
        tracing::debug!(?op);
        match op {
            StreamOp::Reader(ReaderOp::Drop { panic }) => {
                if let Some(reader) = self.reader.take().filter(|_| *panic) {
                    drop_while_panicking(reader);
                }
            }
            StreamOp::Reader(op) => {
                if let Some(reader) = self.reader.as_mut() {
                    return op
                        .apply(reader, &mut self.reader_data, &mut self.reader_buffer)
                        .await;
                }
            }
            StreamOp::Writer(WriterOp::Drop { panic }) => {
                if let Some(writer) = self.writer.take().filter(|_| *panic) {
                    drop_while_panicking(writer);
                }
            }
            StreamOp::Writer(op) => {
                if let Some(writer) = self.writer.as_mut() {
                    return op.apply(writer, &mut self.writer_data).await;
                }
            }
            StreamOp::Sleep(ms) => {
                tokio::time::sleep(Duration::from_millis(*ms as _)).await;
            }
        }

        Ok(())
    }
}

pub trait StreamExt {
    type Reader: AsyncRead + Send + Unpin + 'static;
    type Writer: AsyncWrite + Send + Unpin + 'static;

    fn into_ops(self) -> Split<Self::Reader, Self::Writer>;
}

impl StreamExt for testing::tcp::Stream {
    type Reader = tokio::net::tcp::OwnedReadHalf;
    type Writer = tokio::net::tcp::OwnedWriteHalf;

    fn into_ops(self) -> Split<Self::Reader, Self::Writer> {
        let (reader, writer) = self.into_split();
        Split::new(reader, writer)
    }
}

impl StreamExt for crate::stream::application::Stream<crate::testing::NoopSubscriber> {
    type Reader = crate::stream::recv::application::Reader<crate::testing::NoopSubscriber>;
    type Writer = crate::stream::send::application::Writer<crate::testing::NoopSubscriber>;

    fn into_ops(self) -> Split<Self::Reader, Self::Writer> {
        let (reader, writer) = self.into_split();
        Split::new(reader, writer)
    }
}

struct Context {
    runtime: tokio::runtime::Runtime,
    tcp: Arc<testing::tcp::Context>,
    dcquic_tcp: Arc<AssertUnwindSafe<testing::dcquic::tcp::Context>>,
    dcquic_tcp_enabled: bool,
    dcquic_udp: Arc<AssertUnwindSafe<testing::dcquic::udp::Context>>,
    dcquic_udp_enabled: bool,
}

impl Default for Context {
    fn default() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        let (tcp, dcquic_tcp, dcquic_udp) = runtime.block_on(async {
            (
                Arc::new(testing::tcp::Context::new().await),
                Arc::new(AssertUnwindSafe(testing::dcquic::tcp::Context::new().await)),
                Arc::new(AssertUnwindSafe(testing::dcquic::udp::Context::new().await)),
            )
        });

        let protocols = std::env::var("DIFFERENTIAL_PROTOCOLS").unwrap_or_default();
        let protocols: Vec<_> = protocols.split(',').filter(|v| !v.is_empty()).collect();

        let dcquic_tcp_enabled = protocols.is_empty() || protocols.contains(&"tcp");
        let dcquic_udp_enabled = protocols.is_empty() || protocols.contains(&"udp");

        assert!(
            dcquic_tcp_enabled || dcquic_udp_enabled,
            "dcquic (either TCP or UDP) needs to be enabled for the test"
        );

        Self {
            runtime,
            tcp,
            dcquic_tcp,
            dcquic_tcp_enabled,
            dcquic_udp,
            dcquic_udp_enabled,
        }
    }
}

pub mod outcome {
    pub type Endpoint = Result<Result<Stats, Error>, Error>;

    pub type Outcome = Result<Endpoints, Error>;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[must_use]
    pub struct Endpoints {
        pub client: Endpoint,
        pub server: Endpoint,
    }

    impl Endpoints {
        pub fn did_panic(&self) -> bool {
            let Self { client, server } = self;

            [client, server]
                .iter()
                .filter_map(|res| res.err())
                .any(|err| matches!(err, Error::Panic))
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Stats {
        pub recv: u64,
        pub send: u64,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Error {
        Panic,
        Timeout,
        Io {
            op_index: usize,
            kind: std::io::ErrorKind,
        },
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[must_use]
    pub struct Set {
        pub tcp: Outcome,
        pub dcquic_tcp: Option<Outcome>,
        pub dcquic_udp: Option<Outcome>,
    }

    impl Set {
        pub fn unwrap(self) -> Outcome {
            self.assert_ok();
            self.tcp
        }

        pub fn log_err(&self, input: &super::PairOp) {
            if !self.is_ok() {
                tracing::error!("{input:#?}\nPRODUCED: {:#?}", self);
            }
        }

        pub fn assert_ok(&self) {
            assert!(self.is_ok(), "{self:#?}");
        }

        pub fn is_ok(&self) -> bool {
            let Self {
                tcp,
                dcquic_tcp,
                dcquic_udp,
            } = self;

            // the task should never panic
            let mut matches = !self.did_panic();

            for outcome in [dcquic_tcp, dcquic_udp].into_iter().flatten() {
                matches &= tcp == outcome;
            }

            matches
        }

        pub fn did_panic(&self) -> bool {
            let Self {
                tcp,
                dcquic_tcp,
                dcquic_udp,
            } = self;

            [dcquic_tcp, dcquic_udp]
                .into_iter()
                .flatten()
                .chain(core::iter::once(tcp))
                .any(|outcome| match outcome {
                    Ok(endpoint) => endpoint.did_panic(),
                    Err(err) => matches!(err, Error::Panic),
                })
        }
    }
}

impl Context {
    pub fn run(&self, pair: &PairOp) -> outcome::Set {
        let client_ops = Arc::new(pair.client.clone());
        let server_ops = Arc::new(pair.server.clone());

        async fn apply<S: StreamExt>(
            stream: S,
            ops: Arc<Vec<StreamOp>>,
        ) -> Result<outcome::Stats, outcome::Error> {
            let mut stream = stream.into_ops();
            let stream = stream.apply_all(&ops);
            let stream = timeout(Duration::from_secs(5), stream);

            stream
                .await
                .map_err(|_| outcome::Error::Timeout)
                .and_then(|res| res)
        }

        macro_rules! run {
            ($context:ident) => {{
                let context = self.$context.clone();
                let client_ops = client_ops.clone();
                let server_ops = server_ops.clone();

                let task = tokio::spawn(
                    async move {
                        tracing::trace!(stringify!($context));

                        let (client, server) = context.pair().await;

                        let client = tokio::spawn(
                            apply(client, client_ops).instrument(info_span!("client")),
                        );

                        let server = tokio::spawn(
                            apply(server, server_ops).instrument(info_span!("server")),
                        );

                        let client = client.await.map_err(|_err| outcome::Error::Panic);
                        let server = server.await.map_err(|_err| outcome::Error::Panic);

                        outcome::Endpoints { client, server }
                    }
                    .instrument(info_span!(stringify!($context))),
                );

                async move { task.await.map_err(|_| outcome::Error::Panic) }
            }};
            ($context:ident, $enabled:expr) => {{
                let task = if $enabled { Some(run!($context)) } else { None };

                async move {
                    if let Some(task) = task {
                        Some(task.await)
                    } else {
                        None
                    }
                }
            }};
        }

        self.runtime.block_on(async {
            let tcp = run!(tcp);
            let dcquic_tcp = run!(dcquic_tcp, self.dcquic_tcp_enabled);
            let dcquic_udp = run!(dcquic_udp, self.dcquic_udp_enabled);

            let tcp = tcp.await;
            let dcquic_tcp = dcquic_tcp.await;
            let dcquic_udp = dcquic_udp.await;

            outcome::Set {
                tcp,
                dcquic_tcp,
                dcquic_udp,
            }
        })
    }
}

/// NOTE: because the runtime is created outside of the test, ASAN will detect leaks with this
/// test. These checks need to be disabled by setting `env ASAN_OPTIONS=detect_leaks=0`.
#[test]
#[cfg_attr(not(fuzzing), ignore = "several differences need to be addressed")]
fn test() {
    let find_panics = std::env::var("DIFFERENTIAL_PANIC_ONLY").is_ok();
    let print_errors = std::env::var("DIFFERENTIAL_PRINT_ERRORS").is_ok();

    let context = Context::default();

    bolero::check!()
        .with_type::<PairOp>()
        .with_shrink_time(Duration::from_secs(60))
        .with_test_time(Duration::from_secs(60))
        .for_each(|pair| {
            let outcome = context.run(pair);

            // if we're only looking for panics then just check that
            if find_panics {
                assert!(!outcome.did_panic(), "{outcome:#?}");
                return;
            }

            // if we're wanting to explore any errors then print and keep going
            if print_errors {
                outcome.log_err(pair);
                return;
            }

            // otherwise assert the outcome is correct
            outcome.assert_ok();
        });
}

#[test]
fn request_response_test() {
    let context = Context::default();

    // overread by one byte. otherwise, the OS scheduler may let the socket close
    // before the peer gets a chance to call `shutdown`.
    let overread = 1;

    let ops = PairOp {
        server: vec![
            StreamOp::Reader(ReaderOp::Read(100 + overread)),
            StreamOp::Writer(WriterOp::Write(200)),
            StreamOp::Writer(WriterOp::Shutdown),
        ],
        client: vec![
            StreamOp::Writer(WriterOp::Write(100)),
            StreamOp::Writer(WriterOp::Shutdown),
            StreamOp::Reader(ReaderOp::Read(200 + overread)),
        ],
    };

    let outcome = context.run(&ops);

    insta::assert_debug_snapshot!(outcome.unwrap());
}
