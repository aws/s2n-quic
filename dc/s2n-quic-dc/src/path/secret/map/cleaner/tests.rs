// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::bach::Clock,
    event::{tracing, Subscriber},
    path::secret::{stateless_reset::Signer, Map},
    testing::{ext::*, sim, without_tracing},
};
use ::tracing::info;
use bach::time::Instant;
use std::{
    fs::{self, File},
    io::Write,
    net::SocketAddr,
    path::Path,
    process::Command,
    sync::Arc,
    sync::Mutex,
    time::Duration,
};

#[derive(Clone)]
struct EvictionAge(Log);

impl Subscriber for EvictionAge {
    type ConnectionContext = ();

    #[inline]
    fn create_connection_context(
        &self,
        _meta: &crate::event::api::ConnectionMeta,
        _info: &crate::event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_path_secret_map_id_entry_evicted(
        &self,
        _meta: &crate::event::api::EndpointMeta,
        event: &crate::event::api::PathSecretMapIdEntryEvicted,
    ) {
        let time = Instant::now().elapsed_since_start().as_secs_f32();
        let age = event.age.as_secs_f32();
        writeln!(self.0.lock().unwrap(), "{time},{age}").unwrap();
    }
}

type Log = Arc<Mutex<File>>;

fn new_map<S>(peer_count: usize, subscriber: S) -> Map
where
    S: Subscriber,
{
    let signer = Signer::random();
    let clock = Clock::default();
    let capacity = peer_count * 3;
    let subscriber = (tracing::Subscriber::default(), subscriber);
    let map = Map::new(signer, capacity, clock, subscriber);

    for idx in 0..peer_count {
        let addr = u32::from_be_bytes([10, 0, 0, 0]) + idx as u32;
        let addr = SocketAddr::new(addr.to_be_bytes().into(), 443);
        let map = map.clone();
        let sleep_time = (1..10 * 60).any().s();
        async move {
            sleep_time.sleep().await;
            map.test_insert(addr);
        }
        .spawn();
    }

    let request_handshake = {
        let map = map.clone();
        move |address| {
            info!("handshake {address:?}");
            let map = map.clone();
            async move {
                4.ms().sleep().await;
                map.test_insert(address);
            }
            .spawn();
        }
    };

    map.register_request_handshake(Box::new(request_handshake));

    map
}

#[test]
fn rehandshake_sim() {
    let dir = Path::new("target/sim/rehandshake");
    fs::create_dir_all(dir).unwrap();

    let mut log = File::create(dir.join("events.csv")).unwrap();
    writeln!(log, "time,age").unwrap();

    let log = Arc::new(Mutex::new(log));

    let one_hour = Duration::from_secs(60 * 60);
    let hours = 24 * 7;
    let total_time = one_hour * hours;

    without_tracing(|| {
        sim(|| {
            let client_count = 300_000;
            let server_count = 2;

            for idx in 0..client_count {
                let log = log.clone();
                async move {
                    let subscriber = EvictionAge(log);
                    let map = new_map(server_count, subscriber);
                    total_time.sleep().await;
                    drop(map);
                }
                .group(format!("client-{idx}"))
                .with_seed(idx)
                .spawn();
            }

            async move {
                for _ in 0..hours {
                    for _ in 0..60 {
                        let before = std::time::Instant::now();
                        60.s().sleep().await;
                        let real_time = before.elapsed();
                        let ratio = 60.0 / real_time.as_secs_f64();
                        eprintln!("{} tick - speed={ratio:.2}x", Instant::now());
                    }
                }
            }
            .primary()
            .spawn();
        });
    });

    fs::write(dir.join("script.sql"), SQL_SCRIPT).unwrap();

    macro_rules! cmd {
        ($program:expr $(, $arg:expr)* $(,)?) => {
            let mut p = Command::new($program);
            $(
                p.arg($arg);
            )*
            assert!(p
                .current_dir(dir)
                .status()
                .unwrap()
                .success(), "{} failed", $program);
        };
    }

    cmd!("duckdb", "-s", ".read script.sql");

    fs::write(dir.join("script.plot"), PLOT_SCRIPT).unwrap();

    cmd!("gnuplot", "script.plot");
}

static SQL_SCRIPT: &str = r#"
COPY (
    SELECT
        TRUNC(time / 60) * 60 AS time,
        AVG(age) AS avg_age
    FROM 'events.csv'
    GROUP BY TRUNC(time / 60)
    ORDER BY TRUNC(time / 60)
) TO 'averages.csv' (HEADER, DELIMITER ',');
"#;

static PLOT_SCRIPT: &str = r#"
set datafile separator ','
set terminal png
set output "eviction-age.png"
set xlabel "time (seconds)
set ylabel "age"
set title "Average Eviction Age"
plot "averages.csv" using 1:2 with lines
"#;
