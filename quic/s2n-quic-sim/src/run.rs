// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{stats, Result};
use indicatif::{ParallelProgressIterator, ProgressBar};
use rayon::prelude::*;
use s2n_quic::provider::io::testing::{Model, Test};
use structopt::StructOpt;

mod config;
pub use config::Config;

mod endpoint;
mod events;

mod range;
use range::CliRange;

#[derive(Debug, StructOpt)]
pub struct Run {
    #[structopt(flatten)]
    config: Config,

    #[structopt(long)]
    seed: Vec<u64>,

    #[structopt(long)]
    progress: bool,
}

impl core::ops::Deref for Run {
    type Target = Config;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl Run {
    pub fn run(&self) -> Result {
        assert_ne!(self.servers.start, 0);
        assert_ne!(self.clients.start, 0);
        assert_ne!(self.connections.start, 0);

        let test = |seed: u64| {
            let network = Model::default();

            Test::new(network.clone())
                .with_seed(seed)
                .run(|handle| {
                    let server_len = self.servers.gen();
                    let client_len = self.clients.gen();

                    let events = self.gen_network(seed, server_len, client_len, &network);

                    let mut servers = vec![];
                    for _ in 0..server_len {
                        servers.push(endpoint::server(handle, events.clone())?);
                    }

                    for _ in 0..client_len {
                        let count = self.connections.gen() as usize;
                        let delay = self.connect_delay;
                        let streams = self.streams;
                        let stream_data = self.stream_data;
                        endpoint::client(
                            handle,
                            events.clone(),
                            &servers,
                            count,
                            delay,
                            streams,
                            stream_data,
                        )?;
                    }

                    Ok(())
                })
                .unwrap();
        };

        if self.seed.is_empty() {
            events::dump(|stdout| {
                stats::Setup {
                    args: self.config.args(),
                }
                .write(stdout)
            });

            let pb = if self.progress {
                ProgressBar::new(self.iterations as _)
            } else {
                ProgressBar::hidden()
            };

            static MSG: &str =
                "{spinner:.green} [{elapsed_precise}] [{bar:.cyan/blue}] {pos}/{len} ({eta})";

            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template(MSG)
                    .unwrap()
                    .progress_chars("=> "),
            );

            (0..self.iterations)
                .into_par_iter()
                .progress_with(pb)
                .map(|v| if events::is_open() { Some(v) } else { None })
                .while_some()
                .for_each(|_| {
                    use ::rand::prelude::*;
                    let seed = thread_rng().gen();
                    test(seed);
                });
        } else {
            // don't dump events when running specific seeds
            events::close();

            for seed in self.seed.iter().copied() {
                test(seed);
            }
        }

        Ok(())
    }

    fn gen_network(&self, seed: u64, servers: u32, clients: u32, model: &Model) -> events::Events {
        let mut events = stats::Parameters {
            seed,
            servers,
            clients,
            ..Default::default()
        };

        macro_rules! param {
            ($name:ident, $set:ident, gen_duration) => {{
                let value = self.$name.gen_duration();
                model.$set(value);
                events.$name = Some(value.into());
            }};
            ($name:ident, $set:ident, $gen:ident $($tt:tt)*) => {{
                let value = self.$name.$gen();
                model.$set(value);
                events.$name = value $($tt)*;
            }};
        }

        param!(drop_rate, set_drop_rate, gen * 100.0);
        param!(corrupt_rate, set_corrupt_rate, gen * 100.0);
        param!(jitter, set_jitter, gen_duration);
        param!(network_jitter, set_network_jitter, gen_duration);
        param!(delay, set_delay, gen_duration);
        param!(inflight_delay, set_inflight_delay, gen_duration);
        param!(retransmit_rate, set_retransmit_rate, gen * 100.0);
        param!(max_udp_payload, set_max_udp_payload, gen as _);

        macro_rules! zero_param {
            ($name:ident, $set:ident) => {
                if self.$name.end > 0 {
                    let value = self.$name.gen();
                    model.$set(value);
                    events.$name = value;
                } else {
                    events.$name = u64::MAX;
                }
            };
        }

        zero_param!(transmit_rate, set_transmit_rate);
        zero_param!(max_inflight, set_max_inflight);
        zero_param!(inflight_delay_threshold, set_inflight_delay_threshold);

        events.into()
    }
}
