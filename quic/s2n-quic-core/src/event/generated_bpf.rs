// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use super::{api, bpf::IntoBpf};
#[repr(C)]
#[derive(Debug)]
pub(super) struct RecoveryMetrics {
    pub path: u64,
    pub min_rtt: u64,
    pub smoothed_rtt: u64,
    pub latest_rtt: u64,
    pub rtt_variance: u64,
    pub max_ack_delay: u64,
    pub pto_count: u64,
    pub congestion_window: u64,
    pub bytes_in_flight: u64,
}
impl<'a> IntoBpf<RecoveryMetrics> for api::RecoveryMetrics<'a> {
    #[inline]
    fn as_bpf(&self) -> RecoveryMetrics {
        RecoveryMetrics {
            path: self.path.as_bpf(),
            min_rtt: self.min_rtt.as_bpf(),
            smoothed_rtt: self.smoothed_rtt.as_bpf(),
            latest_rtt: self.latest_rtt.as_bpf(),
            rtt_variance: self.rtt_variance.as_bpf(),
            max_ack_delay: self.max_ack_delay.as_bpf(),
            pto_count: self.pto_count.as_bpf(),
            congestion_window: self.congestion_window.as_bpf(),
            bytes_in_flight: self.bytes_in_flight.as_bpf(),
        }
    }
}
#[repr(C)]
#[derive(Debug)]
pub(super) struct RxStreamProgress {
    pub bytes: u64,
}
impl IntoBpf<RxStreamProgress> for api::RxStreamProgress {
    #[inline]
    fn as_bpf(&self) -> RxStreamProgress {
        RxStreamProgress {
            bytes: self.bytes.as_bpf(),
        }
    }
}
#[repr(C)]
#[derive(Debug)]
pub(super) struct TxStreamProgress {
    pub bytes: u64,
}
impl IntoBpf<TxStreamProgress> for api::TxStreamProgress {
    #[inline]
    fn as_bpf(&self) -> TxStreamProgress {
        TxStreamProgress {
            bytes: self.bytes.as_bpf(),
        }
    }
}
#[repr(C)]
#[derive(Debug)]
pub(super) struct EndpointDatagramReceived {
    pub len: u64,
}
impl IntoBpf<EndpointDatagramReceived> for api::EndpointDatagramReceived {
    #[inline]
    fn as_bpf(&self) -> EndpointDatagramReceived {
        EndpointDatagramReceived {
            len: self.len.as_bpf(),
        }
    }
}
