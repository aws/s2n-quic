// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, structopt::StructOpt)]
pub struct Limits {
    /// The maximum bits/sec for each connection
    #[structopt(long, default_value = "150")]
    pub max_throughput: u64,

    /// The expected RTT in milliseconds
    #[structopt(long, default_value = "100")]
    pub expected_rtt: u64,

    #[structopt(long)]
    pub stream_send_buffer_size: Option<u32>,
}

impl Limits {
    pub fn limits(&self) -> s2n_quic::provider::limits::Limits {
        let data_window = self.data_window();

        let mut limits = s2n_quic::provider::limits::Limits::default();

        limits = limits
            .with_data_window(data_window)
            .unwrap()
            .with_max_send_buffer_size(data_window.min(u32::MAX as _) as _)
            .unwrap()
            .with_bidirectional_local_data_window(data_window)
            .unwrap()
            .with_bidirectional_remote_data_window(data_window)
            .unwrap()
            .with_unidirectional_data_window(data_window)
            .unwrap();

        if let Some(size) = self.stream_send_buffer_size {
            limits = limits.with_max_send_buffer_size(size).unwrap();
        }

        limits
    }

    fn data_window(&self) -> u64 {
        s2n_quic_core::transport::parameters::compute_data_window(
            self.max_throughput,
            core::time::Duration::from_millis(self.expected_rtt),
            2,
        )
        .as_u64()
    }
}
