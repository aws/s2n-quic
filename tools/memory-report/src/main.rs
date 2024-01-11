// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{Client, Server};
use s2n_quic_core::stream::testing::Data;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Snapshot {
    total: u64,
    rss: u64,
    max: u64,
}

macro_rules! assert_event {
    ($event:expr, $actual:expr, $expected:expr) => {{
        assert!(
            $actual < $expected,
            "{}: expected {} to be less than {}",
            $event,
            $actual,
            $expected
        );
    }};
}

impl Snapshot {
    pub fn new() -> Self {
        let stats = dhat::HeapStats::get();
        Self {
            total: stats.total_bytes,
            rss: stats.curr_bytes as _,
            max: stats.max_bytes as _,
        }
    }

    pub fn print_diff(&self, event: &str, streams: usize) {
        let (alloc, rss) = self.diff(Self::new());

        // make some assertions about the amount of memory use
        if streams > 0 {
            let expected = match event {
                "post-handshake" => 12_000,
                "post-transfer" => 30_000,
                "post-close" => 512,
                e => unimplemented!("{}", e),
            };
            assert_event!(event, rss, expected);
        }

        println!("{event}\t{alloc}\t{rss}\t{streams}");
    }

    pub fn diff(&self, other: Self) -> (u64, i64) {
        let alloc = self.alloc_diff(other);
        let rss = self.rss_diff(other);
        (alloc, rss)
    }

    pub fn rss_diff(&self, other: Self) -> i64 {
        let before = self.rss as i64;
        let after = other.rss as i64;
        after - before
    }

    pub fn alloc_diff(&self, other: Self) -> u64 {
        other.total - self.total
    }
}

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T = (), E = Error> = core::result::Result<T, E>;

fn main() -> Result {
    let mut args = std::env::args();
    let _ = args.next();
    let arg = args.next();
    let _profiler = dhat::Profiler::new_heap();
    run(arg.as_deref())
}

#[tokio::main]
async fn run(arg: Option<&str>) -> Result {
    match arg {
        Some("server") => server().await,
        Some("client") => client().await,
        _ => Err("memory-report server|client".into()),
    }
}

async fn client() -> Result {
    let io = ("0.0.0.0", 0);

    let tls = s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM;

    let client = Client::builder()
        .with_io(io)?
        .with_tls(tls)?
        .start()
        .unwrap();

    println!("event\talloc_diff\trss_diff\tstreams");

    for stream_count in 0..10 {
        // wait for a bit to have the allocations settle
        tokio::time::sleep(core::time::Duration::from_millis(1000)).await;

        let snapshot = Snapshot::new();
        let connect = s2n_quic::client::Connect::new(("127.0.0.1".parse()?, 4433))
            .with_server_name("localhost");

        let mut connection = client.connect(connect).await?;

        // wait for a bit to have the allocations settle
        tokio::time::sleep(core::time::Duration::from_millis(1000)).await;

        snapshot.print_diff("post-handshake", stream_count);

        for _ in 0..stream_count {
            let mut stream = connection.open_bidirectional_stream().await?;

            let mut data = Data::new(5 * 1_000_000);

            while let Some(chunk) = data.send_one(usize::MAX) {
                stream.send(chunk).await?;
                // flush the chunk, otherwise we will fill up the send buffer and increase total
                // allocations
                stream.flush().await?;
            }

            stream.close().await?;
        }

        tokio::time::sleep(core::time::Duration::from_millis(5000)).await;

        snapshot.print_diff("post-transfer", stream_count);

        connection.close(123u8.into());
        drop(connection);

        tokio::time::sleep(core::time::Duration::from_millis(5000)).await;

        snapshot.print_diff("post-close", stream_count);
    }

    Ok(())
}

async fn server() -> Result {
    let io = ("127.0.0.1", 4433);

    let tls = (
        s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM,
        s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM,
    );

    let mut server = Server::builder()
        .with_io(io)?
        .with_tls(tls)?
        .start()
        .unwrap();

    eprintln!("Server listening on port {}", io.1);

    while let Some(mut connection) = server.accept().await {
        tokio::spawn(async move {
            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                tokio::spawn(async move {
                    while let Ok(Some(data)) = stream.receive().await {
                        let _ = data;
                    }
                });
            }
        });
    }

    Ok(())
}
