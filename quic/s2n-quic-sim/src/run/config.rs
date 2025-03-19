// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::CliRange;
use jiff::SignedDuration;
use serde::Deserialize;
use structopt::StructOpt;

macro_rules! config {
    (struct Config { $(#[name = $name:literal] #[default = $default:literal] $field:ident: $ty:ty),* $(,)? }) => {
        mod defaults {
            use super::*;
            $(
            pub fn $field() -> $ty {
                $default.parse().unwrap()
            }
            )*
        }
        use defaults::*;

        #[derive(Clone, Debug, StructOpt, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Config {
            $(
                #[structopt(long, default_value = $default)]
                #[serde(default = $name)]
                pub $field: $ty,
            )*
        }

        impl Config {
            pub fn args(&self) -> Vec<String> {
                let mut args = vec![];

                $(
                    if self.$field != defaults::$field() {
                        args.push(format!("--{}", $name.replace('_', "-")));
                        args.push(self.$field.to_string());
                    }
                )*

                args
            }
        }
    };
}

config!(
    struct Config {
        #[name = "drop_rate"]
        #[default = "0.0"]
        drop_rate: CliRange<f64>,

        #[name = "corrupt_rate"]
        #[default = "0.0"]
        corrupt_rate: CliRange<f64>,

        #[name = "jitter"]
        #[default = "0ms"]
        jitter: CliRange<SignedDuration>,

        #[name = "network_jitter"]
        #[default = "0ms"]
        network_jitter: CliRange<SignedDuration>,

        #[name = "delay"]
        #[default = "100ms"]
        delay: CliRange<SignedDuration>,

        #[name = "transmit_rate"]
        #[default = "0"]
        transmit_rate: CliRange<u64>,

        #[name = "retransmit_rate"]
        #[default = "0.0"]
        retransmit_rate: CliRange<f64>,

        #[name = "max_udp_payload"]
        #[default = "1450"]
        max_udp_payload: CliRange<u16>,

        #[name = "max_inflight"]
        #[default = "0"]
        max_inflight: CliRange<u64>,

        #[name = "inflight_delay"]
        #[default = "0ms"]
        inflight_delay: CliRange<SignedDuration>,

        #[name = "inflight_delay_threshold"]
        #[default = "0"]
        inflight_delay_threshold: CliRange<u64>,

        #[name = "clients"]
        #[default = "1"]
        clients: CliRange<u32>,

        #[name = "servers"]
        #[default = "1"]
        servers: CliRange<u32>,

        #[name = "connect_delay"]
        #[default = "0ms"]
        connect_delay: CliRange<SignedDuration>,

        #[name = "connections"]
        #[default = "1"]
        connections: CliRange<u32>,

        #[name = "streams"]
        #[default = "1"]
        streams: CliRange<u32>,

        #[name = "stream_data"]
        #[default = "4096"]
        stream_data: CliRange<u64>,

        #[name = "iterations"]
        #[default = "10000"]
        iterations: u64,
    }
);
