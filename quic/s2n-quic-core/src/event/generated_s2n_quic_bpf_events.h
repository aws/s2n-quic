/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* DO NOT MODIFY THIS FILE
 * This file was generated with the `s2n-quic-events` crate and any required
 * changes should be made there.
 */

#include <linux/path.h>

struct RecoveryMetrics {
  uint64_t path;
  uint64_t min_rtt;
  uint64_t smoothed_rtt;
  uint64_t latest_rtt;
  uint64_t rtt_variance;
  uint64_t max_ack_delay;
  uint64_t pto_count;
  uint64_t congestion_window;
  uint64_t bytes_in_flight;
};
struct RxStreamProgress {
  uint64_t bytes;
};
struct TxStreamProgress {
  uint64_t bytes;
};
struct EndpointDatagramReceived {
  uint64_t len;
};
