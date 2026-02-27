// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsVariant, BoolRecorder, Info, Metric, NominalRecorder, Recorder, Registry, Units,
    },
};
use alloc::{boxed::Box, vec::Vec};
mod id {
    pub const APPLICATION_PROTOCOL_INFORMATION: usize = 0usize;
    pub const SERVER_NAME_INFORMATION: usize = APPLICATION_PROTOCOL_INFORMATION + 1;
    pub const KEY_EXCHANGE_GROUP: usize = SERVER_NAME_INFORMATION + 1;
    pub const PACKET_SKIPPED: usize = KEY_EXCHANGE_GROUP + 1;
    pub const PACKET_SENT: usize = PACKET_SKIPPED + 1;
    pub const PACKET_SENT__KIND: usize = PACKET_SENT + 1;
    pub const PACKET_SENT__BYTES__TOTAL: usize = PACKET_SENT__KIND + 1;
    pub const PACKET_SENT__BYTES: usize = PACKET_SENT__BYTES__TOTAL + 1;
    pub const PACKET_RECEIVED: usize = PACKET_SENT__BYTES + 1;
    pub const PACKET_RECEIVED__KIND: usize = PACKET_RECEIVED + 1;
    pub const ACTIVE_PATH_UPDATED: usize = PACKET_RECEIVED__KIND + 1;
    pub const PATH_CREATED: usize = ACTIVE_PATH_UPDATED + 1;
    pub const FRAME_SENT: usize = PATH_CREATED + 1;
    pub const FRAME_SENT__PACKET: usize = FRAME_SENT + 1;
    pub const FRAME_SENT__FRAME: usize = FRAME_SENT__PACKET + 1;
    pub const FRAME_RECEIVED: usize = FRAME_SENT__FRAME + 1;
    pub const FRAME_RECEIVED__PACKET: usize = FRAME_RECEIVED + 1;
    pub const FRAME_RECEIVED__FRAME: usize = FRAME_RECEIVED__PACKET + 1;
    pub const CONNECTION_CLOSE_FRAME_RECEIVED: usize = FRAME_RECEIVED__FRAME + 1;
    pub const CONNECTION_CLOSE_FRAME_RECEIVED__PACKET: usize = CONNECTION_CLOSE_FRAME_RECEIVED + 1;
    pub const PACKET_LOST: usize = CONNECTION_CLOSE_FRAME_RECEIVED__PACKET + 1;
    pub const PACKET_LOST__KIND: usize = PACKET_LOST + 1;
    pub const PACKET_LOST__BYTES__TOTAL: usize = PACKET_LOST__KIND + 1;
    pub const PACKET_LOST__BYTES: usize = PACKET_LOST__BYTES__TOTAL + 1;
    pub const PACKET_LOST__IS_MTU_PROBE: usize = PACKET_LOST__BYTES + 1;
    pub const RECOVERY_METRICS: usize = PACKET_LOST__IS_MTU_PROBE + 1;
    pub const RECOVERY_METRICS__MIN_RTT: usize = RECOVERY_METRICS + 1;
    pub const RECOVERY_METRICS__SMOOTHED_RTT: usize = RECOVERY_METRICS__MIN_RTT + 1;
    pub const RECOVERY_METRICS__LATEST_RTT: usize = RECOVERY_METRICS__SMOOTHED_RTT + 1;
    pub const RECOVERY_METRICS__RTT_VARIANCE: usize = RECOVERY_METRICS__LATEST_RTT + 1;
    pub const RECOVERY_METRICS__MAX_ACK_DELAY: usize = RECOVERY_METRICS__RTT_VARIANCE + 1;
    pub const RECOVERY_METRICS__PTO_COUNT: usize = RECOVERY_METRICS__MAX_ACK_DELAY + 1;
    pub const RECOVERY_METRICS__CONGESTION_WINDOW: usize = RECOVERY_METRICS__PTO_COUNT + 1;
    pub const RECOVERY_METRICS__BYTES_IN_FLIGHT: usize = RECOVERY_METRICS__CONGESTION_WINDOW + 1;
    pub const RECOVERY_METRICS__CONGESTION_LIMITED: usize = RECOVERY_METRICS__BYTES_IN_FLIGHT + 1;
    pub const CONGESTION: usize = RECOVERY_METRICS__CONGESTION_LIMITED + 1;
    pub const CONGESTION__SOURCE: usize = CONGESTION + 1;
    pub const RX_ACK_RANGE_DROPPED: usize = CONGESTION__SOURCE + 1;
    pub const ACK_RANGE_RECEIVED: usize = RX_ACK_RANGE_DROPPED + 1;
    pub const ACK_RANGE_RECEIVED__PACKET: usize = ACK_RANGE_RECEIVED + 1;
    pub const ACK_RANGE_SENT: usize = ACK_RANGE_RECEIVED__PACKET + 1;
    pub const ACK_RANGE_SENT__PACKET: usize = ACK_RANGE_SENT + 1;
    pub const PACKET_DROPPED: usize = ACK_RANGE_SENT__PACKET + 1;
    pub const PACKET_DROPPED__REASON: usize = PACKET_DROPPED + 1;
    pub const KEY_UPDATE: usize = PACKET_DROPPED__REASON + 1;
    pub const KEY_UPDATE__KEY_TYPE: usize = KEY_UPDATE + 1;
    pub const KEY_UPDATE__CIPHER_SUITE: usize = KEY_UPDATE__KEY_TYPE + 1;
    pub const KEY_SPACE_DISCARDED: usize = KEY_UPDATE__CIPHER_SUITE + 1;
    pub const KEY_SPACE_DISCARDED__INITIAL__LATENCY: usize = KEY_SPACE_DISCARDED + 1;
    pub const KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY: usize =
        KEY_SPACE_DISCARDED__INITIAL__LATENCY + 1;
    pub const KEY_SPACE_DISCARDED__ONE_RTT__LATENCY: usize =
        KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY + 1;
    pub const KEY_SPACE_DISCARDED__SPACE: usize = KEY_SPACE_DISCARDED__ONE_RTT__LATENCY + 1;
    pub const CONNECTION_STARTED: usize = KEY_SPACE_DISCARDED__SPACE + 1;
    pub const DUPLICATE_PACKET: usize = CONNECTION_STARTED + 1;
    pub const DUPLICATE_PACKET__KIND: usize = DUPLICATE_PACKET + 1;
    pub const DUPLICATE_PACKET__ERROR: usize = DUPLICATE_PACKET__KIND + 1;
    pub const TRANSPORT_PARAMETERS_RECEIVED: usize = DUPLICATE_PACKET__ERROR + 1;
    pub const TRANSPORT_PARAMETERS_RECEIVED__LATENCY: usize = TRANSPORT_PARAMETERS_RECEIVED + 1;
    pub const DATAGRAM_SENT: usize = TRANSPORT_PARAMETERS_RECEIVED__LATENCY + 1;
    pub const DATAGRAM_SENT__BYTES__TOTAL: usize = DATAGRAM_SENT + 1;
    pub const DATAGRAM_SENT__BYTES: usize = DATAGRAM_SENT__BYTES__TOTAL + 1;
    pub const DATAGRAM_SENT__GSO_OFFSET: usize = DATAGRAM_SENT__BYTES + 1;
    pub const DATAGRAM_RECEIVED: usize = DATAGRAM_SENT__GSO_OFFSET + 1;
    pub const DATAGRAM_RECEIVED__BYTES__TOTAL: usize = DATAGRAM_RECEIVED + 1;
    pub const DATAGRAM_RECEIVED__BYTES: usize = DATAGRAM_RECEIVED__BYTES__TOTAL + 1;
    pub const DATAGRAM_DROPPED: usize = DATAGRAM_RECEIVED__BYTES + 1;
    pub const DATAGRAM_DROPPED__BYTES__TOTAL: usize = DATAGRAM_DROPPED + 1;
    pub const DATAGRAM_DROPPED__BYTES: usize = DATAGRAM_DROPPED__BYTES__TOTAL + 1;
    pub const DATAGRAM_DROPPED__REASON: usize = DATAGRAM_DROPPED__BYTES + 1;
    pub const HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED: usize = DATAGRAM_DROPPED__REASON + 1;
    pub const CONNECTION_ID_UPDATED: usize = HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED + 1;
    pub const ECN_STATE_CHANGED: usize = CONNECTION_ID_UPDATED + 1;
    pub const ECN_STATE_CHANGED__STATE: usize = ECN_STATE_CHANGED + 1;
    pub const CONNECTION_MIGRATION_DENIED: usize = ECN_STATE_CHANGED__STATE + 1;
    pub const CONNECTION_MIGRATION_DENIED__REASON: usize = CONNECTION_MIGRATION_DENIED + 1;
    pub const HANDSHAKE_STATUS_UPDATED: usize = CONNECTION_MIGRATION_DENIED__REASON + 1;
    pub const HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY: usize = HANDSHAKE_STATUS_UPDATED + 1;
    pub const HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY: usize =
        HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY + 1;
    pub const HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY: usize =
        HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY + 1;
    pub const HANDSHAKE_STATUS_UPDATED__STATUS: usize =
        HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY + 1;
    pub const TLS_EXPORTER_READY: usize = HANDSHAKE_STATUS_UPDATED__STATUS + 1;
    pub const TLS_HANDSHAKE_FAILED: usize = TLS_EXPORTER_READY + 1;
    pub const PATH_CHALLENGE_UPDATED: usize = TLS_HANDSHAKE_FAILED + 1;
    pub const PATH_CHALLENGE_UPDATED__STATUS: usize = PATH_CHALLENGE_UPDATED + 1;
    pub const TLS_CLIENT_HELLO: usize = PATH_CHALLENGE_UPDATED__STATUS + 1;
    pub const TLS_CLIENT_HELLO__LATENCY: usize = TLS_CLIENT_HELLO + 1;
    pub const TLS_SERVER_HELLO: usize = TLS_CLIENT_HELLO__LATENCY + 1;
    pub const TLS_SERVER_HELLO__LATENCY: usize = TLS_SERVER_HELLO + 1;
    pub const RX_STREAM_PROGRESS: usize = TLS_SERVER_HELLO__LATENCY + 1;
    pub const RX_STREAM_PROGRESS__BYTES__TOTAL: usize = RX_STREAM_PROGRESS + 1;
    pub const RX_STREAM_PROGRESS__BYTES: usize = RX_STREAM_PROGRESS__BYTES__TOTAL + 1;
    pub const TX_STREAM_PROGRESS: usize = RX_STREAM_PROGRESS__BYTES + 1;
    pub const TX_STREAM_PROGRESS__BYTES__TOTAL: usize = TX_STREAM_PROGRESS + 1;
    pub const TX_STREAM_PROGRESS__BYTES: usize = TX_STREAM_PROGRESS__BYTES__TOTAL + 1;
    pub const KEEP_ALIVE_TIMER_EXPIRED: usize = TX_STREAM_PROGRESS__BYTES + 1;
    pub const MTU_UPDATED: usize = KEEP_ALIVE_TIMER_EXPIRED + 1;
    pub const MTU_UPDATED__MTU: usize = MTU_UPDATED + 1;
    pub const MTU_UPDATED__CAUSE: usize = MTU_UPDATED__MTU + 1;
    pub const MTU_UPDATED__SEARCH_COMPLETE: usize = MTU_UPDATED__CAUSE + 1;
    pub const MTU_PROBING_COMPLETE_RECEIVED: usize = MTU_UPDATED__SEARCH_COMPLETE + 1;
    pub const MTU_PROBING_COMPLETE_RECEIVED__PACKET: usize = MTU_PROBING_COMPLETE_RECEIVED + 1;
    pub const MTU_PROBING_COMPLETE_RECEIVED__MTU: usize = MTU_PROBING_COMPLETE_RECEIVED__PACKET + 1;
    pub const SLOW_START_EXITED: usize = MTU_PROBING_COMPLETE_RECEIVED__MTU + 1;
    pub const SLOW_START_EXITED__CAUSE: usize = SLOW_START_EXITED + 1;
    pub const SLOW_START_EXITED__LATENCY: usize = SLOW_START_EXITED__CAUSE + 1;
    pub const SLOW_START_EXITED__CONGESTION_WINDOW: usize = SLOW_START_EXITED__LATENCY + 1;
    pub const DELIVERY_RATE_SAMPLED: usize = SLOW_START_EXITED__CONGESTION_WINDOW + 1;
    pub const PACING_RATE_UPDATED: usize = DELIVERY_RATE_SAMPLED + 1;
    pub const PACING_RATE_UPDATED__BYTES_PER_SECOND: usize = PACING_RATE_UPDATED + 1;
    pub const PACING_RATE_UPDATED__BURST_SIZE: usize = PACING_RATE_UPDATED__BYTES_PER_SECOND + 1;
    pub const PACING_RATE_UPDATED__PACING_GAIN: usize = PACING_RATE_UPDATED__BURST_SIZE + 1;
    pub const BBR_STATE_CHANGED: usize = PACING_RATE_UPDATED__PACING_GAIN + 1;
    pub const BBR_STATE_CHANGED__STATE: usize = BBR_STATE_CHANGED + 1;
    pub const DC_STATE_CHANGED: usize = BBR_STATE_CHANGED__STATE + 1;
    pub const DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY: usize = DC_STATE_CHANGED + 1;
    pub const DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY: usize =
        DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY + 1;
    pub const DC_STATE_CHANGED__PATH_SECRETS__LATENCY: usize =
        DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY + 1;
    pub const DC_STATE_CHANGED__COMPLETE__LATENCY: usize =
        DC_STATE_CHANGED__PATH_SECRETS__LATENCY + 1;
    pub const DC_STATE_CHANGED__STATE: usize = DC_STATE_CHANGED__COMPLETE__LATENCY + 1;
    pub const DC_PATH_CREATED: usize = DC_STATE_CHANGED__STATE + 1;
    pub const CONNECTION_CLOSED: usize = DC_PATH_CREATED + 1;
    pub const CONNECTION_CLOSED__LATENCY: usize = CONNECTION_CLOSED + 1;
    pub const CONNECTION_CLOSED__ERROR: usize = CONNECTION_CLOSED__LATENCY + 1;
    pub const VERSION_INFORMATION: usize = CONNECTION_CLOSED__ERROR + 1;
    pub const ENDPOINT_PACKET_SENT: usize = VERSION_INFORMATION + 1;
    pub const ENDPOINT_PACKET_RECEIVED: usize = ENDPOINT_PACKET_SENT + 1;
    pub const ENDPOINT_DATAGRAM_SENT: usize = ENDPOINT_PACKET_RECEIVED + 1;
    pub const ENDPOINT_DATAGRAM_SENT__BYTES: usize = ENDPOINT_DATAGRAM_SENT + 1;
    pub const ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL: usize = ENDPOINT_DATAGRAM_SENT__BYTES + 1;
    pub const ENDPOINT_DATAGRAM_SENT__GSO_OFFSET: usize = ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL + 1;
    pub const ENDPOINT_DATAGRAM_RECEIVED: usize = ENDPOINT_DATAGRAM_SENT__GSO_OFFSET + 1;
    pub const ENDPOINT_DATAGRAM_RECEIVED__BYTES: usize = ENDPOINT_DATAGRAM_RECEIVED + 1;
    pub const ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL: usize =
        ENDPOINT_DATAGRAM_RECEIVED__BYTES + 1;
    pub const ENDPOINT_DATAGRAM_DROPPED: usize = ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL + 1;
    pub const ENDPOINT_DATAGRAM_DROPPED__BYTES: usize = ENDPOINT_DATAGRAM_DROPPED + 1;
    pub const ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL: usize = ENDPOINT_DATAGRAM_DROPPED__BYTES + 1;
    pub const ENDPOINT_DATAGRAM_DROPPED__REASON: usize =
        ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL + 1;
    pub const ENDPOINT_CONNECTION_ATTEMPT_FAILED: usize = ENDPOINT_DATAGRAM_DROPPED__REASON + 1;
    pub const ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR: usize =
        ENDPOINT_CONNECTION_ATTEMPT_FAILED + 1;
    pub const ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED: usize =
        ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR + 1;
    pub const PLATFORM_TX: usize = ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED + 1;
    pub const PLATFORM_TX__PACKETS__TOTAL: usize = PLATFORM_TX + 1;
    pub const PLATFORM_TX__PACKETS: usize = PLATFORM_TX__PACKETS__TOTAL + 1;
    pub const PLATFORM_TX__SYSCALLS__TOTAL: usize = PLATFORM_TX__PACKETS + 1;
    pub const PLATFORM_TX__SYSCALLS: usize = PLATFORM_TX__SYSCALLS__TOTAL + 1;
    pub const PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL: usize = PLATFORM_TX__SYSCALLS + 1;
    pub const PLATFORM_TX__SYSCALLS__BLOCKED: usize = PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL + 1;
    pub const PLATFORM_TX__ERRORS__TOTAL: usize = PLATFORM_TX__SYSCALLS__BLOCKED + 1;
    pub const PLATFORM_TX__ERRORS: usize = PLATFORM_TX__ERRORS__TOTAL + 1;
    pub const PLATFORM_TX__ERRORS__DROPPED__TOTAL: usize = PLATFORM_TX__ERRORS + 1;
    pub const PLATFORM_TX__ERRORS__DROPPED: usize = PLATFORM_TX__ERRORS__DROPPED__TOTAL + 1;
    pub const PLATFORM_TX_ERROR: usize = PLATFORM_TX__ERRORS__DROPPED + 1;
    pub const PLATFORM_RX: usize = PLATFORM_TX_ERROR + 1;
    pub const PLATFORM_RX__PACKETS__TOTAL: usize = PLATFORM_RX + 1;
    pub const PLATFORM_RX__PACKETS: usize = PLATFORM_RX__PACKETS__TOTAL + 1;
    pub const PLATFORM_RX__SYSCALLS__TOTAL: usize = PLATFORM_RX__PACKETS + 1;
    pub const PLATFORM_RX__SYSCALLS: usize = PLATFORM_RX__SYSCALLS__TOTAL + 1;
    pub const PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL: usize = PLATFORM_RX__SYSCALLS + 1;
    pub const PLATFORM_RX__SYSCALLS__BLOCKED: usize = PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL + 1;
    pub const PLATFORM_RX__ERRORS__TOTAL: usize = PLATFORM_RX__SYSCALLS__BLOCKED + 1;
    pub const PLATFORM_RX__ERRORS: usize = PLATFORM_RX__ERRORS__TOTAL + 1;
    pub const PLATFORM_RX__ERRORS__DROPPED__TOTAL: usize = PLATFORM_RX__ERRORS + 1;
    pub const PLATFORM_RX__ERRORS__DROPPED: usize = PLATFORM_RX__ERRORS__DROPPED__TOTAL + 1;
    pub const PLATFORM_RX_ERROR: usize = PLATFORM_RX__ERRORS__DROPPED + 1;
    pub const PLATFORM_FEATURE_CONFIGURED: usize = PLATFORM_RX_ERROR + 1;
    pub const PLATFORM_EVENT_LOOP_WAKEUP: usize = PLATFORM_FEATURE_CONFIGURED + 1;
    pub const PLATFORM_EVENT_LOOP_SLEEP: usize = PLATFORM_EVENT_LOOP_WAKEUP + 1;
    pub const PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION: usize = PLATFORM_EVENT_LOOP_SLEEP + 1;
    pub const PLATFORM_EVENT_LOOP_STARTED: usize =
        PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION + 1;
    pub const COUNTERS_APPLICATION_PROTOCOL_INFORMATION: usize = 0usize;
    pub const COUNTERS_SERVER_NAME_INFORMATION: usize =
        COUNTERS_APPLICATION_PROTOCOL_INFORMATION + 1;
    pub const COUNTERS_KEY_EXCHANGE_GROUP: usize = COUNTERS_SERVER_NAME_INFORMATION + 1;
    pub const COUNTERS_PACKET_SKIPPED: usize = COUNTERS_KEY_EXCHANGE_GROUP + 1;
    pub const COUNTERS_PACKET_SENT: usize = COUNTERS_PACKET_SKIPPED + 1;
    pub const COUNTERS_PACKET_SENT__BYTES__TOTAL: usize = COUNTERS_PACKET_SENT + 1;
    pub const COUNTERS_PACKET_RECEIVED: usize = COUNTERS_PACKET_SENT__BYTES__TOTAL + 1;
    pub const COUNTERS_ACTIVE_PATH_UPDATED: usize = COUNTERS_PACKET_RECEIVED + 1;
    pub const COUNTERS_PATH_CREATED: usize = COUNTERS_ACTIVE_PATH_UPDATED + 1;
    pub const COUNTERS_FRAME_SENT: usize = COUNTERS_PATH_CREATED + 1;
    pub const COUNTERS_FRAME_RECEIVED: usize = COUNTERS_FRAME_SENT + 1;
    pub const COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED: usize = COUNTERS_FRAME_RECEIVED + 1;
    pub const COUNTERS_PACKET_LOST: usize = COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED + 1;
    pub const COUNTERS_PACKET_LOST__BYTES__TOTAL: usize = COUNTERS_PACKET_LOST + 1;
    pub const COUNTERS_RECOVERY_METRICS: usize = COUNTERS_PACKET_LOST__BYTES__TOTAL + 1;
    pub const COUNTERS_CONGESTION: usize = COUNTERS_RECOVERY_METRICS + 1;
    pub const COUNTERS_RX_ACK_RANGE_DROPPED: usize = COUNTERS_CONGESTION + 1;
    pub const COUNTERS_ACK_RANGE_RECEIVED: usize = COUNTERS_RX_ACK_RANGE_DROPPED + 1;
    pub const COUNTERS_ACK_RANGE_SENT: usize = COUNTERS_ACK_RANGE_RECEIVED + 1;
    pub const COUNTERS_PACKET_DROPPED: usize = COUNTERS_ACK_RANGE_SENT + 1;
    pub const COUNTERS_KEY_UPDATE: usize = COUNTERS_PACKET_DROPPED + 1;
    pub const COUNTERS_KEY_SPACE_DISCARDED: usize = COUNTERS_KEY_UPDATE + 1;
    pub const COUNTERS_CONNECTION_STARTED: usize = COUNTERS_KEY_SPACE_DISCARDED + 1;
    pub const COUNTERS_DUPLICATE_PACKET: usize = COUNTERS_CONNECTION_STARTED + 1;
    pub const COUNTERS_TRANSPORT_PARAMETERS_RECEIVED: usize = COUNTERS_DUPLICATE_PACKET + 1;
    pub const COUNTERS_DATAGRAM_SENT: usize = COUNTERS_TRANSPORT_PARAMETERS_RECEIVED + 1;
    pub const COUNTERS_DATAGRAM_SENT__BYTES__TOTAL: usize = COUNTERS_DATAGRAM_SENT + 1;
    pub const COUNTERS_DATAGRAM_RECEIVED: usize = COUNTERS_DATAGRAM_SENT__BYTES__TOTAL + 1;
    pub const COUNTERS_DATAGRAM_RECEIVED__BYTES__TOTAL: usize = COUNTERS_DATAGRAM_RECEIVED + 1;
    pub const COUNTERS_DATAGRAM_DROPPED: usize = COUNTERS_DATAGRAM_RECEIVED__BYTES__TOTAL + 1;
    pub const COUNTERS_DATAGRAM_DROPPED__BYTES__TOTAL: usize = COUNTERS_DATAGRAM_DROPPED + 1;
    pub const COUNTERS_HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED: usize =
        COUNTERS_DATAGRAM_DROPPED__BYTES__TOTAL + 1;
    pub const COUNTERS_CONNECTION_ID_UPDATED: usize =
        COUNTERS_HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED + 1;
    pub const COUNTERS_ECN_STATE_CHANGED: usize = COUNTERS_CONNECTION_ID_UPDATED + 1;
    pub const COUNTERS_CONNECTION_MIGRATION_DENIED: usize = COUNTERS_ECN_STATE_CHANGED + 1;
    pub const COUNTERS_HANDSHAKE_STATUS_UPDATED: usize = COUNTERS_CONNECTION_MIGRATION_DENIED + 1;
    pub const COUNTERS_TLS_EXPORTER_READY: usize = COUNTERS_HANDSHAKE_STATUS_UPDATED + 1;
    pub const COUNTERS_TLS_HANDSHAKE_FAILED: usize = COUNTERS_TLS_EXPORTER_READY + 1;
    pub const COUNTERS_PATH_CHALLENGE_UPDATED: usize = COUNTERS_TLS_HANDSHAKE_FAILED + 1;
    pub const COUNTERS_TLS_CLIENT_HELLO: usize = COUNTERS_PATH_CHALLENGE_UPDATED + 1;
    pub const COUNTERS_TLS_SERVER_HELLO: usize = COUNTERS_TLS_CLIENT_HELLO + 1;
    pub const COUNTERS_RX_STREAM_PROGRESS: usize = COUNTERS_TLS_SERVER_HELLO + 1;
    pub const COUNTERS_RX_STREAM_PROGRESS__BYTES__TOTAL: usize = COUNTERS_RX_STREAM_PROGRESS + 1;
    pub const COUNTERS_TX_STREAM_PROGRESS: usize = COUNTERS_RX_STREAM_PROGRESS__BYTES__TOTAL + 1;
    pub const COUNTERS_TX_STREAM_PROGRESS__BYTES__TOTAL: usize = COUNTERS_TX_STREAM_PROGRESS + 1;
    pub const COUNTERS_KEEP_ALIVE_TIMER_EXPIRED: usize =
        COUNTERS_TX_STREAM_PROGRESS__BYTES__TOTAL + 1;
    pub const COUNTERS_MTU_UPDATED: usize = COUNTERS_KEEP_ALIVE_TIMER_EXPIRED + 1;
    pub const COUNTERS_MTU_PROBING_COMPLETE_RECEIVED: usize = COUNTERS_MTU_UPDATED + 1;
    pub const COUNTERS_SLOW_START_EXITED: usize = COUNTERS_MTU_PROBING_COMPLETE_RECEIVED + 1;
    pub const COUNTERS_DELIVERY_RATE_SAMPLED: usize = COUNTERS_SLOW_START_EXITED + 1;
    pub const COUNTERS_PACING_RATE_UPDATED: usize = COUNTERS_DELIVERY_RATE_SAMPLED + 1;
    pub const COUNTERS_BBR_STATE_CHANGED: usize = COUNTERS_PACING_RATE_UPDATED + 1;
    pub const COUNTERS_DC_STATE_CHANGED: usize = COUNTERS_BBR_STATE_CHANGED + 1;
    pub const COUNTERS_DC_PATH_CREATED: usize = COUNTERS_DC_STATE_CHANGED + 1;
    pub const COUNTERS_CONNECTION_CLOSED: usize = COUNTERS_DC_PATH_CREATED + 1;
    pub const COUNTERS_VERSION_INFORMATION: usize = COUNTERS_CONNECTION_CLOSED + 1;
    pub const COUNTERS_ENDPOINT_PACKET_SENT: usize = COUNTERS_VERSION_INFORMATION + 1;
    pub const COUNTERS_ENDPOINT_PACKET_RECEIVED: usize = COUNTERS_ENDPOINT_PACKET_SENT + 1;
    pub const COUNTERS_ENDPOINT_DATAGRAM_SENT: usize = COUNTERS_ENDPOINT_PACKET_RECEIVED + 1;
    pub const COUNTERS_ENDPOINT_DATAGRAM_RECEIVED: usize = COUNTERS_ENDPOINT_DATAGRAM_SENT + 1;
    pub const COUNTERS_ENDPOINT_DATAGRAM_DROPPED: usize = COUNTERS_ENDPOINT_DATAGRAM_RECEIVED + 1;
    pub const COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED: usize =
        COUNTERS_ENDPOINT_DATAGRAM_DROPPED + 1;
    pub const COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED: usize =
        COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED + 1;
    pub const COUNTERS_PLATFORM_TX: usize = COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED + 1;
    pub const COUNTERS_PLATFORM_TX__PACKETS__TOTAL: usize = COUNTERS_PLATFORM_TX + 1;
    pub const COUNTERS_PLATFORM_TX__SYSCALLS__TOTAL: usize =
        COUNTERS_PLATFORM_TX__PACKETS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL: usize =
        COUNTERS_PLATFORM_TX__SYSCALLS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_TX__ERRORS__TOTAL: usize =
        COUNTERS_PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL + 1;
    pub const COUNTERS_PLATFORM_TX__ERRORS__DROPPED__TOTAL: usize =
        COUNTERS_PLATFORM_TX__ERRORS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_TX_ERROR: usize = COUNTERS_PLATFORM_TX__ERRORS__DROPPED__TOTAL + 1;
    pub const COUNTERS_PLATFORM_RX: usize = COUNTERS_PLATFORM_TX_ERROR + 1;
    pub const COUNTERS_PLATFORM_RX__PACKETS__TOTAL: usize = COUNTERS_PLATFORM_RX + 1;
    pub const COUNTERS_PLATFORM_RX__SYSCALLS__TOTAL: usize =
        COUNTERS_PLATFORM_RX__PACKETS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL: usize =
        COUNTERS_PLATFORM_RX__SYSCALLS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_RX__ERRORS__TOTAL: usize =
        COUNTERS_PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL + 1;
    pub const COUNTERS_PLATFORM_RX__ERRORS__DROPPED__TOTAL: usize =
        COUNTERS_PLATFORM_RX__ERRORS__TOTAL + 1;
    pub const COUNTERS_PLATFORM_RX_ERROR: usize = COUNTERS_PLATFORM_RX__ERRORS__DROPPED__TOTAL + 1;
    pub const COUNTERS_PLATFORM_FEATURE_CONFIGURED: usize = COUNTERS_PLATFORM_RX_ERROR + 1;
    pub const COUNTERS_PLATFORM_EVENT_LOOP_WAKEUP: usize = COUNTERS_PLATFORM_FEATURE_CONFIGURED + 1;
    pub const COUNTERS_PLATFORM_EVENT_LOOP_SLEEP: usize = COUNTERS_PLATFORM_EVENT_LOOP_WAKEUP + 1;
    pub const COUNTERS_PLATFORM_EVENT_LOOP_STARTED: usize = COUNTERS_PLATFORM_EVENT_LOOP_SLEEP + 1;
    pub const BOOL_COUNTERS_PACKET_LOST__IS_MTU_PROBE: usize = 0usize;
    pub const BOOL_COUNTERS_RECOVERY_METRICS__CONGESTION_LIMITED: usize =
        BOOL_COUNTERS_PACKET_LOST__IS_MTU_PROBE + 1;
    pub const BOOL_COUNTERS_MTU_UPDATED__SEARCH_COMPLETE: usize =
        BOOL_COUNTERS_RECOVERY_METRICS__CONGESTION_LIMITED + 1;
    pub const NOMINAL_COUNTERS_PACKET_SENT__KIND: usize = 0usize;
    pub const NOMINAL_COUNTERS_PACKET_RECEIVED__KIND: usize =
        NOMINAL_COUNTERS_PACKET_SENT__KIND + 1;
    pub const NOMINAL_COUNTERS_FRAME_SENT__PACKET: usize =
        NOMINAL_COUNTERS_PACKET_RECEIVED__KIND + 1;
    pub const NOMINAL_COUNTERS_FRAME_SENT__FRAME: usize = NOMINAL_COUNTERS_FRAME_SENT__PACKET + 1;
    pub const NOMINAL_COUNTERS_FRAME_RECEIVED__PACKET: usize =
        NOMINAL_COUNTERS_FRAME_SENT__FRAME + 1;
    pub const NOMINAL_COUNTERS_FRAME_RECEIVED__FRAME: usize =
        NOMINAL_COUNTERS_FRAME_RECEIVED__PACKET + 1;
    pub const NOMINAL_COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED__PACKET: usize =
        NOMINAL_COUNTERS_FRAME_RECEIVED__FRAME + 1;
    pub const NOMINAL_COUNTERS_PACKET_LOST__KIND: usize =
        NOMINAL_COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED__PACKET + 1;
    pub const NOMINAL_COUNTERS_CONGESTION__SOURCE: usize = NOMINAL_COUNTERS_PACKET_LOST__KIND + 1;
    pub const NOMINAL_COUNTERS_ACK_RANGE_RECEIVED__PACKET: usize =
        NOMINAL_COUNTERS_CONGESTION__SOURCE + 1;
    pub const NOMINAL_COUNTERS_ACK_RANGE_SENT__PACKET: usize =
        NOMINAL_COUNTERS_ACK_RANGE_RECEIVED__PACKET + 1;
    pub const NOMINAL_COUNTERS_PACKET_DROPPED__REASON: usize =
        NOMINAL_COUNTERS_ACK_RANGE_SENT__PACKET + 1;
    pub const NOMINAL_COUNTERS_KEY_UPDATE__KEY_TYPE: usize =
        NOMINAL_COUNTERS_PACKET_DROPPED__REASON + 1;
    pub const NOMINAL_COUNTERS_KEY_UPDATE__CIPHER_SUITE: usize =
        NOMINAL_COUNTERS_KEY_UPDATE__KEY_TYPE + 1;
    pub const NOMINAL_COUNTERS_KEY_SPACE_DISCARDED__SPACE: usize =
        NOMINAL_COUNTERS_KEY_UPDATE__CIPHER_SUITE + 1;
    pub const NOMINAL_COUNTERS_DUPLICATE_PACKET__KIND: usize =
        NOMINAL_COUNTERS_KEY_SPACE_DISCARDED__SPACE + 1;
    pub const NOMINAL_COUNTERS_DUPLICATE_PACKET__ERROR: usize =
        NOMINAL_COUNTERS_DUPLICATE_PACKET__KIND + 1;
    pub const NOMINAL_COUNTERS_DATAGRAM_DROPPED__REASON: usize =
        NOMINAL_COUNTERS_DUPLICATE_PACKET__ERROR + 1;
    pub const NOMINAL_COUNTERS_ECN_STATE_CHANGED__STATE: usize =
        NOMINAL_COUNTERS_DATAGRAM_DROPPED__REASON + 1;
    pub const NOMINAL_COUNTERS_CONNECTION_MIGRATION_DENIED__REASON: usize =
        NOMINAL_COUNTERS_ECN_STATE_CHANGED__STATE + 1;
    pub const NOMINAL_COUNTERS_HANDSHAKE_STATUS_UPDATED__STATUS: usize =
        NOMINAL_COUNTERS_CONNECTION_MIGRATION_DENIED__REASON + 1;
    pub const NOMINAL_COUNTERS_PATH_CHALLENGE_UPDATED__STATUS: usize =
        NOMINAL_COUNTERS_HANDSHAKE_STATUS_UPDATED__STATUS + 1;
    pub const NOMINAL_COUNTERS_MTU_UPDATED__CAUSE: usize =
        NOMINAL_COUNTERS_PATH_CHALLENGE_UPDATED__STATUS + 1;
    pub const NOMINAL_COUNTERS_MTU_PROBING_COMPLETE_RECEIVED__PACKET: usize =
        NOMINAL_COUNTERS_MTU_UPDATED__CAUSE + 1;
    pub const NOMINAL_COUNTERS_SLOW_START_EXITED__CAUSE: usize =
        NOMINAL_COUNTERS_MTU_PROBING_COMPLETE_RECEIVED__PACKET + 1;
    pub const NOMINAL_COUNTERS_BBR_STATE_CHANGED__STATE: usize =
        NOMINAL_COUNTERS_SLOW_START_EXITED__CAUSE + 1;
    pub const NOMINAL_COUNTERS_DC_STATE_CHANGED__STATE: usize =
        NOMINAL_COUNTERS_BBR_STATE_CHANGED__STATE + 1;
    pub const NOMINAL_COUNTERS_CONNECTION_CLOSED__ERROR: usize =
        NOMINAL_COUNTERS_DC_STATE_CHANGED__STATE + 1;
    pub const NOMINAL_COUNTERS_ENDPOINT_DATAGRAM_DROPPED__REASON: usize =
        NOMINAL_COUNTERS_CONNECTION_CLOSED__ERROR + 1;
    pub const NOMINAL_COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR: usize =
        NOMINAL_COUNTERS_ENDPOINT_DATAGRAM_DROPPED__REASON + 1;
    pub const MEASURES_PACKET_SENT__BYTES: usize = 0usize;
    pub const MEASURES_PACKET_LOST__BYTES: usize = MEASURES_PACKET_SENT__BYTES + 1;
    pub const MEASURES_RECOVERY_METRICS__MIN_RTT: usize = MEASURES_PACKET_LOST__BYTES + 1;
    pub const MEASURES_RECOVERY_METRICS__SMOOTHED_RTT: usize =
        MEASURES_RECOVERY_METRICS__MIN_RTT + 1;
    pub const MEASURES_RECOVERY_METRICS__LATEST_RTT: usize =
        MEASURES_RECOVERY_METRICS__SMOOTHED_RTT + 1;
    pub const MEASURES_RECOVERY_METRICS__RTT_VARIANCE: usize =
        MEASURES_RECOVERY_METRICS__LATEST_RTT + 1;
    pub const MEASURES_RECOVERY_METRICS__MAX_ACK_DELAY: usize =
        MEASURES_RECOVERY_METRICS__RTT_VARIANCE + 1;
    pub const MEASURES_RECOVERY_METRICS__PTO_COUNT: usize =
        MEASURES_RECOVERY_METRICS__MAX_ACK_DELAY + 1;
    pub const MEASURES_RECOVERY_METRICS__CONGESTION_WINDOW: usize =
        MEASURES_RECOVERY_METRICS__PTO_COUNT + 1;
    pub const MEASURES_RECOVERY_METRICS__BYTES_IN_FLIGHT: usize =
        MEASURES_RECOVERY_METRICS__CONGESTION_WINDOW + 1;
    pub const MEASURES_DATAGRAM_SENT__BYTES: usize = MEASURES_RECOVERY_METRICS__BYTES_IN_FLIGHT + 1;
    pub const MEASURES_DATAGRAM_SENT__GSO_OFFSET: usize = MEASURES_DATAGRAM_SENT__BYTES + 1;
    pub const MEASURES_DATAGRAM_RECEIVED__BYTES: usize = MEASURES_DATAGRAM_SENT__GSO_OFFSET + 1;
    pub const MEASURES_DATAGRAM_DROPPED__BYTES: usize = MEASURES_DATAGRAM_RECEIVED__BYTES + 1;
    pub const MEASURES_RX_STREAM_PROGRESS__BYTES: usize = MEASURES_DATAGRAM_DROPPED__BYTES + 1;
    pub const MEASURES_TX_STREAM_PROGRESS__BYTES: usize = MEASURES_RX_STREAM_PROGRESS__BYTES + 1;
    pub const MEASURES_MTU_UPDATED__MTU: usize = MEASURES_TX_STREAM_PROGRESS__BYTES + 1;
    pub const MEASURES_MTU_PROBING_COMPLETE_RECEIVED__MTU: usize = MEASURES_MTU_UPDATED__MTU + 1;
    pub const MEASURES_SLOW_START_EXITED__CONGESTION_WINDOW: usize =
        MEASURES_MTU_PROBING_COMPLETE_RECEIVED__MTU + 1;
    pub const MEASURES_PACING_RATE_UPDATED__BYTES_PER_SECOND: usize =
        MEASURES_SLOW_START_EXITED__CONGESTION_WINDOW + 1;
    pub const MEASURES_PACING_RATE_UPDATED__BURST_SIZE: usize =
        MEASURES_PACING_RATE_UPDATED__BYTES_PER_SECOND + 1;
    pub const MEASURES_PACING_RATE_UPDATED__PACING_GAIN: usize =
        MEASURES_PACING_RATE_UPDATED__BURST_SIZE + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES: usize =
        MEASURES_PACING_RATE_UPDATED__PACING_GAIN + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL: usize =
        MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_SENT__GSO_OFFSET: usize =
        MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES: usize =
        MEASURES_ENDPOINT_DATAGRAM_SENT__GSO_OFFSET + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL: usize =
        MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES: usize =
        MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL + 1;
    pub const MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL: usize =
        MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES + 1;
    pub const MEASURES_PLATFORM_TX__PACKETS: usize =
        MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL + 1;
    pub const MEASURES_PLATFORM_TX__SYSCALLS: usize = MEASURES_PLATFORM_TX__PACKETS + 1;
    pub const MEASURES_PLATFORM_TX__SYSCALLS__BLOCKED: usize = MEASURES_PLATFORM_TX__SYSCALLS + 1;
    pub const MEASURES_PLATFORM_TX__ERRORS: usize = MEASURES_PLATFORM_TX__SYSCALLS__BLOCKED + 1;
    pub const MEASURES_PLATFORM_TX__ERRORS__DROPPED: usize = MEASURES_PLATFORM_TX__ERRORS + 1;
    pub const MEASURES_PLATFORM_RX__PACKETS: usize = MEASURES_PLATFORM_TX__ERRORS__DROPPED + 1;
    pub const MEASURES_PLATFORM_RX__SYSCALLS: usize = MEASURES_PLATFORM_RX__PACKETS + 1;
    pub const MEASURES_PLATFORM_RX__SYSCALLS__BLOCKED: usize = MEASURES_PLATFORM_RX__SYSCALLS + 1;
    pub const MEASURES_PLATFORM_RX__ERRORS: usize = MEASURES_PLATFORM_RX__SYSCALLS__BLOCKED + 1;
    pub const MEASURES_PLATFORM_RX__ERRORS__DROPPED: usize = MEASURES_PLATFORM_RX__ERRORS + 1;
    pub const TIMERS_KEY_SPACE_DISCARDED__INITIAL__LATENCY: usize = 0usize;
    pub const TIMERS_KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY: usize =
        TIMERS_KEY_SPACE_DISCARDED__INITIAL__LATENCY + 1;
    pub const TIMERS_KEY_SPACE_DISCARDED__ONE_RTT__LATENCY: usize =
        TIMERS_KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY + 1;
    pub const TIMERS_TRANSPORT_PARAMETERS_RECEIVED__LATENCY: usize =
        TIMERS_KEY_SPACE_DISCARDED__ONE_RTT__LATENCY + 1;
    pub const TIMERS_HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY: usize =
        TIMERS_TRANSPORT_PARAMETERS_RECEIVED__LATENCY + 1;
    pub const TIMERS_HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY: usize =
        TIMERS_HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY + 1;
    pub const TIMERS_HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY: usize =
        TIMERS_HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY + 1;
    pub const TIMERS_TLS_CLIENT_HELLO__LATENCY: usize =
        TIMERS_HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY + 1;
    pub const TIMERS_TLS_SERVER_HELLO__LATENCY: usize = TIMERS_TLS_CLIENT_HELLO__LATENCY + 1;
    pub const TIMERS_DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY: usize =
        TIMERS_TLS_SERVER_HELLO__LATENCY + 1;
    pub const TIMERS_DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY: usize =
        TIMERS_DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY + 1;
    pub const TIMERS_DC_STATE_CHANGED__PATH_SECRETS__LATENCY: usize =
        TIMERS_DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY + 1;
    pub const TIMERS_DC_STATE_CHANGED__COMPLETE__LATENCY: usize =
        TIMERS_DC_STATE_CHANGED__PATH_SECRETS__LATENCY + 1;
    pub const TIMERS_CONNECTION_CLOSED__LATENCY: usize =
        TIMERS_DC_STATE_CHANGED__COMPLETE__LATENCY + 1;
    pub const TIMERS_PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION: usize =
        TIMERS_CONNECTION_CLOSED__LATENCY + 1;
    pub const NOMINAL_TIMERS_SLOW_START_EXITED__LATENCY: usize = 0usize;
}
static INFO: &[Info; 169usize] = &[
    info::Builder {
        id: id::APPLICATION_PROTOCOL_INFORMATION,
        name: Str::new("application_protocol_information\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::SERVER_NAME_INFORMATION,
        name: Str::new("server_name_information\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_EXCHANGE_GROUP,
        name: Str::new("key_exchange_group\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_SKIPPED,
        name: Str::new("packet_skipped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_SENT,
        name: Str::new("packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_SENT__KIND,
        name: Str::new("packet_sent.kind\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_SENT__BYTES__TOTAL,
        name: Str::new("packet_sent.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACKET_SENT__BYTES,
        name: Str::new("packet_sent.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACKET_RECEIVED,
        name: Str::new("packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_RECEIVED__KIND,
        name: Str::new("packet_received.kind\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ACTIVE_PATH_UPDATED,
        name: Str::new("active_path_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PATH_CREATED,
        name: Str::new("path_created\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_SENT,
        name: Str::new("frame_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_SENT__PACKET,
        name: Str::new("frame_sent.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_SENT__FRAME,
        name: Str::new("frame_sent.frame\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_RECEIVED,
        name: Str::new("frame_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_RECEIVED__PACKET,
        name: Str::new("frame_received.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::FRAME_RECEIVED__FRAME,
        name: Str::new("frame_received.frame\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_CLOSE_FRAME_RECEIVED,
        name: Str::new("connection_close_frame_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET,
        name: Str::new("connection_close_frame_received.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_LOST,
        name: Str::new("packet_lost\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_LOST__KIND,
        name: Str::new("packet_lost.kind\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_LOST__BYTES__TOTAL,
        name: Str::new("packet_lost.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACKET_LOST__BYTES,
        name: Str::new("packet_lost.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACKET_LOST__IS_MTU_PROBE,
        name: Str::new("packet_lost.is_mtu_probe\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS,
        name: Str::new("recovery_metrics\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__MIN_RTT,
        name: Str::new("recovery_metrics.min_rtt\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__SMOOTHED_RTT,
        name: Str::new("recovery_metrics.smoothed_rtt\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__LATEST_RTT,
        name: Str::new("recovery_metrics.latest_rtt\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__RTT_VARIANCE,
        name: Str::new("recovery_metrics.rtt_variance\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__MAX_ACK_DELAY,
        name: Str::new("recovery_metrics.max_ack_delay\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__PTO_COUNT,
        name: Str::new("recovery_metrics.pto_count\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__CONGESTION_WINDOW,
        name: Str::new("recovery_metrics.congestion_window\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__BYTES_IN_FLIGHT,
        name: Str::new("recovery_metrics.bytes_in_flight\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RECOVERY_METRICS__CONGESTION_LIMITED,
        name: Str::new("recovery_metrics.congestion_limited\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONGESTION,
        name: Str::new("congestion\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONGESTION__SOURCE,
        name: Str::new("congestion.source\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::RX_ACK_RANGE_DROPPED,
        name: Str::new("rx_ack_range_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ACK_RANGE_RECEIVED,
        name: Str::new("ack_range_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ACK_RANGE_RECEIVED__PACKET,
        name: Str::new("ack_range_received.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ACK_RANGE_SENT,
        name: Str::new("ack_range_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ACK_RANGE_SENT__PACKET,
        name: Str::new("ack_range_sent.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_DROPPED,
        name: Str::new("packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACKET_DROPPED__REASON,
        name: Str::new("packet_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_UPDATE,
        name: Str::new("key_update\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_UPDATE__KEY_TYPE,
        name: Str::new("key_update.key_type\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_UPDATE__CIPHER_SUITE,
        name: Str::new("key_update.cipher_suite\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_SPACE_DISCARDED,
        name: Str::new("key_space_discarded\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::KEY_SPACE_DISCARDED__INITIAL__LATENCY,
        name: Str::new("key_space_discarded.initial.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY,
        name: Str::new("key_space_discarded.handshake.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY,
        name: Str::new("key_space_discarded.one_rtt.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::KEY_SPACE_DISCARDED__SPACE,
        name: Str::new("key_space_discarded.space\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_STARTED,
        name: Str::new("connection_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DUPLICATE_PACKET,
        name: Str::new("duplicate_packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DUPLICATE_PACKET__KIND,
        name: Str::new("duplicate_packet.kind\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DUPLICATE_PACKET__ERROR,
        name: Str::new("duplicate_packet.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TRANSPORT_PARAMETERS_RECEIVED,
        name: Str::new("transport_parameters_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TRANSPORT_PARAMETERS_RECEIVED__LATENCY,
        name: Str::new("transport_parameters_received.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_SENT,
        name: Str::new("datagram_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_SENT__BYTES__TOTAL,
        name: Str::new("datagram_sent.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_SENT__BYTES,
        name: Str::new("datagram_sent.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_SENT__GSO_OFFSET,
        name: Str::new("datagram_sent.gso_offset\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_RECEIVED,
        name: Str::new("datagram_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_RECEIVED__BYTES__TOTAL,
        name: Str::new("datagram_received.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_RECEIVED__BYTES,
        name: Str::new("datagram_received.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_DROPPED,
        name: Str::new("datagram_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_DROPPED__BYTES__TOTAL,
        name: Str::new("datagram_dropped.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_DROPPED__BYTES,
        name: Str::new("datagram_dropped.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DATAGRAM_DROPPED__REASON,
        name: Str::new("datagram_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED,
        name: Str::new("handshake_remote_address_change_observed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_ID_UPDATED,
        name: Str::new("connection_id_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ECN_STATE_CHANGED,
        name: Str::new("ecn_state_changed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ECN_STATE_CHANGED__STATE,
        name: Str::new("ecn_state_changed.state\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_MIGRATION_DENIED,
        name: Str::new("connection_migration_denied\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_MIGRATION_DENIED__REASON,
        name: Str::new("connection_migration_denied.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_STATUS_UPDATED,
        name: Str::new("handshake_status_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY,
        name: Str::new("handshake_status_updated.complete.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY,
        name: Str::new("handshake_status_updated.confirmed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY,
        name: Str::new("handshake_status_updated.handshake_done_acked.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::HANDSHAKE_STATUS_UPDATED__STATUS,
        name: Str::new("handshake_status_updated.status\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TLS_EXPORTER_READY,
        name: Str::new("tls_exporter_ready\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TLS_HANDSHAKE_FAILED,
        name: Str::new("tls_handshake_failed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PATH_CHALLENGE_UPDATED,
        name: Str::new("path_challenge_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PATH_CHALLENGE_UPDATED__STATUS,
        name: Str::new("path_challenge_updated.status\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TLS_CLIENT_HELLO,
        name: Str::new("tls_client_hello\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TLS_CLIENT_HELLO__LATENCY,
        name: Str::new("tls_client_hello.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::TLS_SERVER_HELLO,
        name: Str::new("tls_server_hello\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TLS_SERVER_HELLO__LATENCY,
        name: Str::new("tls_server_hello.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::RX_STREAM_PROGRESS,
        name: Str::new("rx_stream_progress\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::RX_STREAM_PROGRESS__BYTES__TOTAL,
        name: Str::new("rx_stream_progress.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::RX_STREAM_PROGRESS__BYTES,
        name: Str::new("rx_stream_progress.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::TX_STREAM_PROGRESS,
        name: Str::new("tx_stream_progress\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::TX_STREAM_PROGRESS__BYTES__TOTAL,
        name: Str::new("tx_stream_progress.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::TX_STREAM_PROGRESS__BYTES,
        name: Str::new("tx_stream_progress.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::KEEP_ALIVE_TIMER_EXPIRED,
        name: Str::new("keep_alive_timer_expired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_UPDATED,
        name: Str::new("mtu_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_UPDATED__MTU,
        name: Str::new("mtu_updated.mtu\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::MTU_UPDATED__CAUSE,
        name: Str::new("mtu_updated.cause\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_UPDATED__SEARCH_COMPLETE,
        name: Str::new("mtu_updated.search_complete\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_PROBING_COMPLETE_RECEIVED,
        name: Str::new("mtu_probing_complete_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_PROBING_COMPLETE_RECEIVED__PACKET,
        name: Str::new("mtu_probing_complete_received.packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::MTU_PROBING_COMPLETE_RECEIVED__MTU,
        name: Str::new("mtu_probing_complete_received.mtu\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::SLOW_START_EXITED,
        name: Str::new("slow_start_exited\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::SLOW_START_EXITED__CAUSE,
        name: Str::new("slow_start_exited.cause\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::SLOW_START_EXITED__LATENCY,
        name: Str::new("slow_start_exited.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::SLOW_START_EXITED__CONGESTION_WINDOW,
        name: Str::new("slow_start_exited.congestion_window\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::DELIVERY_RATE_SAMPLED,
        name: Str::new("delivery_rate_sampled\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACING_RATE_UPDATED,
        name: Str::new("pacing_rate_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PACING_RATE_UPDATED__BYTES_PER_SECOND,
        name: Str::new("pacing_rate_updated.bytes_per_second\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACING_RATE_UPDATED__BURST_SIZE,
        name: Str::new("pacing_rate_updated.burst_size\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::PACING_RATE_UPDATED__PACING_GAIN,
        name: Str::new("pacing_rate_updated.pacing_gain\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::BBR_STATE_CHANGED,
        name: Str::new("bbr_state_changed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::BBR_STATE_CHANGED__STATE,
        name: Str::new("bbr_state_changed.state\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED,
        name: Str::new("dc_state_changed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY,
        name: Str::new("dc_state_changed.version_negotiated.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY,
        name: Str::new("dc_state_changed.no_version_negotiated.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED__PATH_SECRETS__LATENCY,
        name: Str::new("dc_state_changed.path_secrets.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED__COMPLETE__LATENCY,
        name: Str::new("dc_state_changed.complete.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::DC_STATE_CHANGED__STATE,
        name: Str::new("dc_state_changed.state\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::DC_PATH_CREATED,
        name: Str::new("dc_path_created\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_CLOSED,
        name: Str::new("connection_closed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_CLOSED__LATENCY,
        name: Str::new("connection_closed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::CONNECTION_CLOSED__ERROR,
        name: Str::new("connection_closed.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::VERSION_INFORMATION,
        name: Str::new("version_information\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_PACKET_SENT,
        name: Str::new("endpoint_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_PACKET_RECEIVED,
        name: Str::new("endpoint_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_SENT,
        name: Str::new("endpoint_datagram_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_SENT__BYTES,
        name: Str::new("endpoint_datagram_sent.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL,
        name: Str::new("endpoint_datagram_sent.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET,
        name: Str::new("endpoint_datagram_sent.gso_offset\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_RECEIVED,
        name: Str::new("endpoint_datagram_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_RECEIVED__BYTES,
        name: Str::new("endpoint_datagram_received.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL,
        name: Str::new("endpoint_datagram_received.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_DROPPED,
        name: Str::new("endpoint_datagram_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_DROPPED__BYTES,
        name: Str::new("endpoint_datagram_dropped.bytes\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL,
        name: Str::new("endpoint_datagram_dropped.bytes.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_DATAGRAM_DROPPED__REASON,
        name: Str::new("endpoint_datagram_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_CONNECTION_ATTEMPT_FAILED,
        name: Str::new("endpoint_connection_attempt_failed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR,
        name: Str::new("endpoint_connection_attempt_failed.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED,
        name: Str::new("endpoint_connection_attempt_deduplicated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX,
        name: Str::new("platform_tx\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__PACKETS__TOTAL,
        name: Str::new("platform_tx.packets.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__PACKETS,
        name: Str::new("platform_tx.packets\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__SYSCALLS__TOTAL,
        name: Str::new("platform_tx.syscalls.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__SYSCALLS,
        name: Str::new("platform_tx.syscalls\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL,
        name: Str::new("platform_tx.syscalls.blocked.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__SYSCALLS__BLOCKED,
        name: Str::new("platform_tx.syscalls.blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__ERRORS__TOTAL,
        name: Str::new("platform_tx.errors.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__ERRORS,
        name: Str::new("platform_tx.errors\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__ERRORS__DROPPED__TOTAL,
        name: Str::new("platform_tx.errors.dropped.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX__ERRORS__DROPPED,
        name: Str::new("platform_tx.errors.dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_TX_ERROR,
        name: Str::new("platform_tx_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX,
        name: Str::new("platform_rx\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__PACKETS__TOTAL,
        name: Str::new("platform_rx.packets.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__PACKETS,
        name: Str::new("platform_rx.packets\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__SYSCALLS__TOTAL,
        name: Str::new("platform_rx.syscalls.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__SYSCALLS,
        name: Str::new("platform_rx.syscalls\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL,
        name: Str::new("platform_rx.syscalls.blocked.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__SYSCALLS__BLOCKED,
        name: Str::new("platform_rx.syscalls.blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__ERRORS__TOTAL,
        name: Str::new("platform_rx.errors.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__ERRORS,
        name: Str::new("platform_rx.errors\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__ERRORS__DROPPED__TOTAL,
        name: Str::new("platform_rx.errors.dropped.total\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX__ERRORS__DROPPED,
        name: Str::new("platform_rx.errors.dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_RX_ERROR,
        name: Str::new("platform_rx_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_FEATURE_CONFIGURED,
        name: Str::new("platform_feature_configured\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_EVENT_LOOP_WAKEUP,
        name: Str::new("platform_event_loop_wakeup\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_EVENT_LOOP_SLEEP,
        name: Str::new("platform_event_loop_sleep\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION,
        name: Str::new("platform_event_loop_sleep.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: id::PLATFORM_EVENT_LOOP_STARTED,
        name: Str::new("platform_event_loop_started\0"),
        units: Units::None,
    }
    .build(),
];
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConnectionContext {
    start_time: crate::event::Timestamp,
}
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 81usize]>,
    #[allow(dead_code)]
    bool_counters: Box<[R::BoolCounter; 3usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_counter_offsets: Box<[usize; 30usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 39usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 15usize]>,
    #[allow(dead_code)]
    nominal_timers: Box<[R::NominalTimer]>,
    #[allow(dead_code)]
    nominal_timer_offsets: Box<[usize; 1usize]>,
    #[allow(dead_code)]
    registry: R,
}
impl<R: Registry + Default> Default for Subscriber<R> {
    fn default() -> Self {
        Self::new(R::default())
    }
}
impl<R: Registry> Subscriber<R> {
    #[doc = r" Creates a new subscriber with the given registry"]
    #[doc = r""]
    #[doc = r" # Note"]
    #[doc = r""]
    #[doc = r" All of the recorders are registered on initialization and cached for the lifetime"]
    #[doc = r" of the subscriber."]
    #[allow(unused_mut)]
    #[inline]
    pub fn new(registry: R) -> Self {
        let mut counters = Vec::with_capacity(81usize);
        let mut bool_counters = Vec::with_capacity(3usize);
        let mut nominal_counters = Vec::with_capacity(30usize);
        let mut nominal_counter_offsets = Vec::with_capacity(30usize);
        let mut measures = Vec::with_capacity(39usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(15usize);
        let mut nominal_timers = Vec::with_capacity(1usize);
        let mut nominal_timer_offsets = Vec::with_capacity(1usize);
        counters.push(registry.register_counter(&INFO[id::APPLICATION_PROTOCOL_INFORMATION]));
        counters.push(registry.register_counter(&INFO[id::SERVER_NAME_INFORMATION]));
        counters.push(registry.register_counter(&INFO[id::KEY_EXCHANGE_GROUP]));
        counters.push(registry.register_counter(&INFO[id::PACKET_SKIPPED]));
        counters.push(registry.register_counter(&INFO[id::PACKET_SENT]));
        counters.push(registry.register_counter(&INFO[id::PACKET_SENT__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PACKET_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::ACTIVE_PATH_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::PATH_CREATED]));
        counters.push(registry.register_counter(&INFO[id::FRAME_SENT]));
        counters.push(registry.register_counter(&INFO[id::FRAME_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::CONNECTION_CLOSE_FRAME_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::PACKET_LOST]));
        counters.push(registry.register_counter(&INFO[id::PACKET_LOST__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::RECOVERY_METRICS]));
        counters.push(registry.register_counter(&INFO[id::CONGESTION]));
        counters.push(registry.register_counter(&INFO[id::RX_ACK_RANGE_DROPPED]));
        counters.push(registry.register_counter(&INFO[id::ACK_RANGE_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::ACK_RANGE_SENT]));
        counters.push(registry.register_counter(&INFO[id::PACKET_DROPPED]));
        counters.push(registry.register_counter(&INFO[id::KEY_UPDATE]));
        counters.push(registry.register_counter(&INFO[id::KEY_SPACE_DISCARDED]));
        counters.push(registry.register_counter(&INFO[id::CONNECTION_STARTED]));
        counters.push(registry.register_counter(&INFO[id::DUPLICATE_PACKET]));
        counters.push(registry.register_counter(&INFO[id::TRANSPORT_PARAMETERS_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_SENT]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_SENT__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_RECEIVED__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_DROPPED]));
        counters.push(registry.register_counter(&INFO[id::DATAGRAM_DROPPED__BYTES__TOTAL]));
        counters
            .push(registry.register_counter(&INFO[id::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED]));
        counters.push(registry.register_counter(&INFO[id::CONNECTION_ID_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::ECN_STATE_CHANGED]));
        counters.push(registry.register_counter(&INFO[id::CONNECTION_MIGRATION_DENIED]));
        counters.push(registry.register_counter(&INFO[id::HANDSHAKE_STATUS_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::TLS_EXPORTER_READY]));
        counters.push(registry.register_counter(&INFO[id::TLS_HANDSHAKE_FAILED]));
        counters.push(registry.register_counter(&INFO[id::PATH_CHALLENGE_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::TLS_CLIENT_HELLO]));
        counters.push(registry.register_counter(&INFO[id::TLS_SERVER_HELLO]));
        counters.push(registry.register_counter(&INFO[id::RX_STREAM_PROGRESS]));
        counters.push(registry.register_counter(&INFO[id::RX_STREAM_PROGRESS__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::TX_STREAM_PROGRESS]));
        counters.push(registry.register_counter(&INFO[id::TX_STREAM_PROGRESS__BYTES__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::KEEP_ALIVE_TIMER_EXPIRED]));
        counters.push(registry.register_counter(&INFO[id::MTU_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::MTU_PROBING_COMPLETE_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::SLOW_START_EXITED]));
        counters.push(registry.register_counter(&INFO[id::DELIVERY_RATE_SAMPLED]));
        counters.push(registry.register_counter(&INFO[id::PACING_RATE_UPDATED]));
        counters.push(registry.register_counter(&INFO[id::BBR_STATE_CHANGED]));
        counters.push(registry.register_counter(&INFO[id::DC_STATE_CHANGED]));
        counters.push(registry.register_counter(&INFO[id::DC_PATH_CREATED]));
        counters.push(registry.register_counter(&INFO[id::CONNECTION_CLOSED]));
        counters.push(registry.register_counter(&INFO[id::VERSION_INFORMATION]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_PACKET_SENT]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_PACKET_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_DATAGRAM_SENT]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_DATAGRAM_RECEIVED]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_DATAGRAM_DROPPED]));
        counters.push(registry.register_counter(&INFO[id::ENDPOINT_CONNECTION_ATTEMPT_FAILED]));
        counters
            .push(registry.register_counter(&INFO[id::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX__PACKETS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX__SYSCALLS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX__ERRORS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX__ERRORS__DROPPED__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_TX_ERROR]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX__PACKETS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX__SYSCALLS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX__ERRORS__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX__ERRORS__DROPPED__TOTAL]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_RX_ERROR]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_FEATURE_CONFIGURED]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_EVENT_LOOP_WAKEUP]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_EVENT_LOOP_SLEEP]));
        counters.push(registry.register_counter(&INFO[id::PLATFORM_EVENT_LOOP_STARTED]));
        bool_counters.push(registry.register_bool_counter(&INFO[id::PACKET_LOST__IS_MTU_PROBE]));
        bool_counters
            .push(registry.register_bool_counter(&INFO[id::RECOVERY_METRICS__CONGESTION_LIMITED]));
        bool_counters.push(registry.register_bool_counter(&INFO[id::MTU_UPDATED__SEARCH_COMPLETE]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::PACKET_SENT__KIND], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::PACKET_RECEIVED__KIND], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::FRAME_SENT__PACKET], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <Frame as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::FRAME_SENT__FRAME], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::FRAME_RECEIVED__PACKET], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <Frame as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::FRAME_RECEIVED__FRAME], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::PACKET_LOST__KIND], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <CongestionSource as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::CONGESTION__SOURCE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(
                            &INFO[id::ACK_RANGE_RECEIVED__PACKET],
                            variant,
                        ),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::ACK_RANGE_SENT__PACKET], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::PACKET_DROPPED__REASON], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <KeyType as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::KEY_UPDATE__KEY_TYPE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <CipherSuite as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::KEY_UPDATE__CIPHER_SUITE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <KeySpace as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(
                            &INFO[id::KEY_SPACE_DISCARDED__SPACE],
                            variant,
                        ),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::DUPLICATE_PACKET__KIND], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DuplicatePacketError as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::DUPLICATE_PACKET__ERROR], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DatagramDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::DATAGRAM_DROPPED__REASON], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <EcnState as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::ECN_STATE_CHANGED__STATE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MigrationDenyReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::CONNECTION_MIGRATION_DENIED__REASON],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <HandshakeStatus as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::HANDSHAKE_STATUS_UPDATED__STATUS],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PathChallengeStatus as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::PATH_CHALLENGE_UPDATED__STATUS],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MtuUpdatedCause as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry.register_nominal_counter(&INFO[id::MTU_UPDATED__CAUSE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <PacketHeader as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::MTU_PROBING_COMPLETE_RECEIVED__PACKET],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SlowStartExitCause as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::SLOW_START_EXITED__CAUSE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <BbrState as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::BBR_STATE_CHANGED__STATE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DcState as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::DC_STATE_CHANGED__STATE], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <crate::connection::Error as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(
                        registry
                            .register_nominal_counter(&INFO[id::CONNECTION_CLOSED__ERROR], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <DatagramDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::ENDPOINT_DATAGRAM_DROPPED__REASON],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <crate::connection::Error as AsVariant>::VARIANTS.iter() {
                    nominal_counters.push(registry.register_nominal_counter(
                        &INFO[id::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR],
                        variant,
                    ));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[id::PACKET_SENT__BYTES]));
        measures.push(registry.register_measure(&INFO[id::PACKET_LOST__BYTES]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__MIN_RTT]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__SMOOTHED_RTT]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__LATEST_RTT]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__RTT_VARIANCE]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__MAX_ACK_DELAY]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__PTO_COUNT]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__CONGESTION_WINDOW]));
        measures.push(registry.register_measure(&INFO[id::RECOVERY_METRICS__BYTES_IN_FLIGHT]));
        measures.push(registry.register_measure(&INFO[id::DATAGRAM_SENT__BYTES]));
        measures.push(registry.register_measure(&INFO[id::DATAGRAM_SENT__GSO_OFFSET]));
        measures.push(registry.register_measure(&INFO[id::DATAGRAM_RECEIVED__BYTES]));
        measures.push(registry.register_measure(&INFO[id::DATAGRAM_DROPPED__BYTES]));
        measures.push(registry.register_measure(&INFO[id::RX_STREAM_PROGRESS__BYTES]));
        measures.push(registry.register_measure(&INFO[id::TX_STREAM_PROGRESS__BYTES]));
        measures.push(registry.register_measure(&INFO[id::MTU_UPDATED__MTU]));
        measures.push(registry.register_measure(&INFO[id::MTU_PROBING_COMPLETE_RECEIVED__MTU]));
        measures.push(registry.register_measure(&INFO[id::SLOW_START_EXITED__CONGESTION_WINDOW]));
        measures.push(registry.register_measure(&INFO[id::PACING_RATE_UPDATED__BYTES_PER_SECOND]));
        measures.push(registry.register_measure(&INFO[id::PACING_RATE_UPDATED__BURST_SIZE]));
        measures.push(registry.register_measure(&INFO[id::PACING_RATE_UPDATED__PACING_GAIN]));
        measures.push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_SENT__BYTES]));
        measures.push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL]));
        measures.push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET]));
        measures.push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_RECEIVED__BYTES]));
        measures
            .push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL]));
        measures.push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_DROPPED__BYTES]));
        measures
            .push(registry.register_measure(&INFO[id::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_TX__PACKETS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_TX__SYSCALLS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_TX__SYSCALLS__BLOCKED]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_TX__ERRORS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_TX__ERRORS__DROPPED]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_RX__PACKETS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_RX__SYSCALLS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_RX__SYSCALLS__BLOCKED]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_RX__ERRORS]));
        measures.push(registry.register_measure(&INFO[id::PLATFORM_RX__ERRORS__DROPPED]));
        timers.push(registry.register_timer(&INFO[id::KEY_SPACE_DISCARDED__INITIAL__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::TRANSPORT_PARAMETERS_RECEIVED__LATENCY]));
        timers
            .push(registry.register_timer(&INFO[id::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY]));
        timers
            .push(registry.register_timer(&INFO[id::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY]));
        timers
            .push(registry.register_timer(
                &INFO[id::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY],
            ));
        timers.push(registry.register_timer(&INFO[id::TLS_CLIENT_HELLO__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::TLS_SERVER_HELLO__LATENCY]));
        timers.push(
            registry.register_timer(&INFO[id::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY]),
        );
        timers.push(
            registry.register_timer(&INFO[id::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY]),
        );
        timers.push(registry.register_timer(&INFO[id::DC_STATE_CHANGED__PATH_SECRETS__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::DC_STATE_CHANGED__COMPLETE__LATENCY]));
        timers.push(registry.register_timer(&INFO[id::CONNECTION_CLOSED__LATENCY]));
        timers.push(
            registry.register_timer(&INFO[id::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION]),
        );
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_timers.len();
                let mut count = 0;
                for variant in <SlowStartExitCause as AsVariant>::VARIANTS.iter() {
                    nominal_timers.push(
                        registry
                            .register_nominal_timer(&INFO[id::SLOW_START_EXITED__LATENCY], variant),
                    );
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_timer_offsets.push(offset);
            }
        }
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            bool_counters: bool_counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            nominal_counters: nominal_counters.into(),
            nominal_counter_offsets: nominal_counter_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            measures: measures
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            gauges: gauges.try_into().unwrap_or_else(|_| panic!("invalid len")),
            timers: timers.try_into().unwrap_or_else(|_| panic!("invalid len")),
            nominal_timers: nominal_timers.into(),
            nominal_timer_offsets: nominal_timer_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            registry,
        }
    }
    #[doc = r" Returns all of the registered counters"]
    #[inline]
    pub fn counters(&self) -> impl Iterator<Item = (&'static Info, &R::Counter)> + '_ {
        self.counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                id::COUNTERS_APPLICATION_PROTOCOL_INFORMATION => {
                    (&INFO[id::APPLICATION_PROTOCOL_INFORMATION], entry)
                }
                id::COUNTERS_SERVER_NAME_INFORMATION => (&INFO[id::SERVER_NAME_INFORMATION], entry),
                id::COUNTERS_KEY_EXCHANGE_GROUP => (&INFO[id::KEY_EXCHANGE_GROUP], entry),
                id::COUNTERS_PACKET_SKIPPED => (&INFO[id::PACKET_SKIPPED], entry),
                id::COUNTERS_PACKET_SENT => (&INFO[id::PACKET_SENT], entry),
                id::COUNTERS_PACKET_SENT__BYTES__TOTAL => {
                    (&INFO[id::PACKET_SENT__BYTES__TOTAL], entry)
                }
                id::COUNTERS_PACKET_RECEIVED => (&INFO[id::PACKET_RECEIVED], entry),
                id::COUNTERS_ACTIVE_PATH_UPDATED => (&INFO[id::ACTIVE_PATH_UPDATED], entry),
                id::COUNTERS_PATH_CREATED => (&INFO[id::PATH_CREATED], entry),
                id::COUNTERS_FRAME_SENT => (&INFO[id::FRAME_SENT], entry),
                id::COUNTERS_FRAME_RECEIVED => (&INFO[id::FRAME_RECEIVED], entry),
                id::COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED => {
                    (&INFO[id::CONNECTION_CLOSE_FRAME_RECEIVED], entry)
                }
                id::COUNTERS_PACKET_LOST => (&INFO[id::PACKET_LOST], entry),
                id::COUNTERS_PACKET_LOST__BYTES__TOTAL => {
                    (&INFO[id::PACKET_LOST__BYTES__TOTAL], entry)
                }
                id::COUNTERS_RECOVERY_METRICS => (&INFO[id::RECOVERY_METRICS], entry),
                id::COUNTERS_CONGESTION => (&INFO[id::CONGESTION], entry),
                id::COUNTERS_RX_ACK_RANGE_DROPPED => (&INFO[id::RX_ACK_RANGE_DROPPED], entry),
                id::COUNTERS_ACK_RANGE_RECEIVED => (&INFO[id::ACK_RANGE_RECEIVED], entry),
                id::COUNTERS_ACK_RANGE_SENT => (&INFO[id::ACK_RANGE_SENT], entry),
                id::COUNTERS_PACKET_DROPPED => (&INFO[id::PACKET_DROPPED], entry),
                id::COUNTERS_KEY_UPDATE => (&INFO[id::KEY_UPDATE], entry),
                id::COUNTERS_KEY_SPACE_DISCARDED => (&INFO[id::KEY_SPACE_DISCARDED], entry),
                id::COUNTERS_CONNECTION_STARTED => (&INFO[id::CONNECTION_STARTED], entry),
                id::COUNTERS_DUPLICATE_PACKET => (&INFO[id::DUPLICATE_PACKET], entry),
                id::COUNTERS_TRANSPORT_PARAMETERS_RECEIVED => {
                    (&INFO[id::TRANSPORT_PARAMETERS_RECEIVED], entry)
                }
                id::COUNTERS_DATAGRAM_SENT => (&INFO[id::DATAGRAM_SENT], entry),
                id::COUNTERS_DATAGRAM_SENT__BYTES__TOTAL => {
                    (&INFO[id::DATAGRAM_SENT__BYTES__TOTAL], entry)
                }
                id::COUNTERS_DATAGRAM_RECEIVED => (&INFO[id::DATAGRAM_RECEIVED], entry),
                id::COUNTERS_DATAGRAM_RECEIVED__BYTES__TOTAL => {
                    (&INFO[id::DATAGRAM_RECEIVED__BYTES__TOTAL], entry)
                }
                id::COUNTERS_DATAGRAM_DROPPED => (&INFO[id::DATAGRAM_DROPPED], entry),
                id::COUNTERS_DATAGRAM_DROPPED__BYTES__TOTAL => {
                    (&INFO[id::DATAGRAM_DROPPED__BYTES__TOTAL], entry)
                }
                id::COUNTERS_HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED => {
                    (&INFO[id::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED], entry)
                }
                id::COUNTERS_CONNECTION_ID_UPDATED => (&INFO[id::CONNECTION_ID_UPDATED], entry),
                id::COUNTERS_ECN_STATE_CHANGED => (&INFO[id::ECN_STATE_CHANGED], entry),
                id::COUNTERS_CONNECTION_MIGRATION_DENIED => {
                    (&INFO[id::CONNECTION_MIGRATION_DENIED], entry)
                }
                id::COUNTERS_HANDSHAKE_STATUS_UPDATED => {
                    (&INFO[id::HANDSHAKE_STATUS_UPDATED], entry)
                }
                id::COUNTERS_TLS_EXPORTER_READY => (&INFO[id::TLS_EXPORTER_READY], entry),
                id::COUNTERS_TLS_HANDSHAKE_FAILED => (&INFO[id::TLS_HANDSHAKE_FAILED], entry),
                id::COUNTERS_PATH_CHALLENGE_UPDATED => (&INFO[id::PATH_CHALLENGE_UPDATED], entry),
                id::COUNTERS_TLS_CLIENT_HELLO => (&INFO[id::TLS_CLIENT_HELLO], entry),
                id::COUNTERS_TLS_SERVER_HELLO => (&INFO[id::TLS_SERVER_HELLO], entry),
                id::COUNTERS_RX_STREAM_PROGRESS => (&INFO[id::RX_STREAM_PROGRESS], entry),
                id::COUNTERS_RX_STREAM_PROGRESS__BYTES__TOTAL => {
                    (&INFO[id::RX_STREAM_PROGRESS__BYTES__TOTAL], entry)
                }
                id::COUNTERS_TX_STREAM_PROGRESS => (&INFO[id::TX_STREAM_PROGRESS], entry),
                id::COUNTERS_TX_STREAM_PROGRESS__BYTES__TOTAL => {
                    (&INFO[id::TX_STREAM_PROGRESS__BYTES__TOTAL], entry)
                }
                id::COUNTERS_KEEP_ALIVE_TIMER_EXPIRED => {
                    (&INFO[id::KEEP_ALIVE_TIMER_EXPIRED], entry)
                }
                id::COUNTERS_MTU_UPDATED => (&INFO[id::MTU_UPDATED], entry),
                id::COUNTERS_MTU_PROBING_COMPLETE_RECEIVED => {
                    (&INFO[id::MTU_PROBING_COMPLETE_RECEIVED], entry)
                }
                id::COUNTERS_SLOW_START_EXITED => (&INFO[id::SLOW_START_EXITED], entry),
                id::COUNTERS_DELIVERY_RATE_SAMPLED => (&INFO[id::DELIVERY_RATE_SAMPLED], entry),
                id::COUNTERS_PACING_RATE_UPDATED => (&INFO[id::PACING_RATE_UPDATED], entry),
                id::COUNTERS_BBR_STATE_CHANGED => (&INFO[id::BBR_STATE_CHANGED], entry),
                id::COUNTERS_DC_STATE_CHANGED => (&INFO[id::DC_STATE_CHANGED], entry),
                id::COUNTERS_DC_PATH_CREATED => (&INFO[id::DC_PATH_CREATED], entry),
                id::COUNTERS_CONNECTION_CLOSED => (&INFO[id::CONNECTION_CLOSED], entry),
                id::COUNTERS_VERSION_INFORMATION => (&INFO[id::VERSION_INFORMATION], entry),
                id::COUNTERS_ENDPOINT_PACKET_SENT => (&INFO[id::ENDPOINT_PACKET_SENT], entry),
                id::COUNTERS_ENDPOINT_PACKET_RECEIVED => {
                    (&INFO[id::ENDPOINT_PACKET_RECEIVED], entry)
                }
                id::COUNTERS_ENDPOINT_DATAGRAM_SENT => (&INFO[id::ENDPOINT_DATAGRAM_SENT], entry),
                id::COUNTERS_ENDPOINT_DATAGRAM_RECEIVED => {
                    (&INFO[id::ENDPOINT_DATAGRAM_RECEIVED], entry)
                }
                id::COUNTERS_ENDPOINT_DATAGRAM_DROPPED => {
                    (&INFO[id::ENDPOINT_DATAGRAM_DROPPED], entry)
                }
                id::COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED => {
                    (&INFO[id::ENDPOINT_CONNECTION_ATTEMPT_FAILED], entry)
                }
                id::COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED => {
                    (&INFO[id::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED], entry)
                }
                id::COUNTERS_PLATFORM_TX => (&INFO[id::PLATFORM_TX], entry),
                id::COUNTERS_PLATFORM_TX__PACKETS__TOTAL => {
                    (&INFO[id::PLATFORM_TX__PACKETS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_TX__SYSCALLS__TOTAL => {
                    (&INFO[id::PLATFORM_TX__SYSCALLS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL => {
                    (&INFO[id::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_TX__ERRORS__TOTAL => {
                    (&INFO[id::PLATFORM_TX__ERRORS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_TX__ERRORS__DROPPED__TOTAL => {
                    (&INFO[id::PLATFORM_TX__ERRORS__DROPPED__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_TX_ERROR => (&INFO[id::PLATFORM_TX_ERROR], entry),
                id::COUNTERS_PLATFORM_RX => (&INFO[id::PLATFORM_RX], entry),
                id::COUNTERS_PLATFORM_RX__PACKETS__TOTAL => {
                    (&INFO[id::PLATFORM_RX__PACKETS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_RX__SYSCALLS__TOTAL => {
                    (&INFO[id::PLATFORM_RX__SYSCALLS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL => {
                    (&INFO[id::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_RX__ERRORS__TOTAL => {
                    (&INFO[id::PLATFORM_RX__ERRORS__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_RX__ERRORS__DROPPED__TOTAL => {
                    (&INFO[id::PLATFORM_RX__ERRORS__DROPPED__TOTAL], entry)
                }
                id::COUNTERS_PLATFORM_RX_ERROR => (&INFO[id::PLATFORM_RX_ERROR], entry),
                id::COUNTERS_PLATFORM_FEATURE_CONFIGURED => {
                    (&INFO[id::PLATFORM_FEATURE_CONFIGURED], entry)
                }
                id::COUNTERS_PLATFORM_EVENT_LOOP_WAKEUP => {
                    (&INFO[id::PLATFORM_EVENT_LOOP_WAKEUP], entry)
                }
                id::COUNTERS_PLATFORM_EVENT_LOOP_SLEEP => {
                    (&INFO[id::PLATFORM_EVENT_LOOP_SLEEP], entry)
                }
                id::COUNTERS_PLATFORM_EVENT_LOOP_STARTED => {
                    (&INFO[id::PLATFORM_EVENT_LOOP_STARTED], entry)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let counter = &self.counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered bool counters"]
    #[inline]
    pub fn bool_counters(&self) -> impl Iterator<Item = (&'static Info, &R::BoolCounter)> + '_ {
        self.bool_counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                id::BOOL_COUNTERS_PACKET_LOST__IS_MTU_PROBE => {
                    (&INFO[id::PACKET_LOST__IS_MTU_PROBE], entry)
                }
                id::BOOL_COUNTERS_RECOVERY_METRICS__CONGESTION_LIMITED => {
                    (&INFO[id::RECOVERY_METRICS__CONGESTION_LIMITED], entry)
                }
                id::BOOL_COUNTERS_MTU_UPDATED__SEARCH_COMPLETE => {
                    (&INFO[id::MTU_UPDATED__SEARCH_COMPLETE], entry)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_bool(&self, info: usize, id: usize, value: bool) {
        let info = &INFO[info];
        let counter = &self.bool_counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered nominal counters"]
    #[inline]
    pub fn nominal_counters(
        &self,
    ) -> impl Iterator<Item = (&'static Info, &[R::NominalCounter], &[info::Variant])> + '_ {
        #[allow(unused_imports)]
        use api::*;
        self.nominal_counter_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                id::NOMINAL_COUNTERS_PACKET_SENT__KIND => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::PACKET_SENT__KIND], entries, variants)
                }
                id::NOMINAL_COUNTERS_PACKET_RECEIVED__KIND => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::PACKET_RECEIVED__KIND], entries, variants)
                }
                id::NOMINAL_COUNTERS_FRAME_SENT__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::FRAME_SENT__PACKET], entries, variants)
                }
                id::NOMINAL_COUNTERS_FRAME_SENT__FRAME => {
                    let offset = *entry;
                    let variants = <Frame as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::FRAME_SENT__FRAME], entries, variants)
                }
                id::NOMINAL_COUNTERS_FRAME_RECEIVED__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::FRAME_RECEIVED__PACKET], entries, variants)
                }
                id::NOMINAL_COUNTERS_FRAME_RECEIVED__FRAME => {
                    let offset = *entry;
                    let variants = <Frame as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::FRAME_RECEIVED__FRAME], entries, variants)
                }
                id::NOMINAL_COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET],
                        entries,
                        variants,
                    )
                }
                id::NOMINAL_COUNTERS_PACKET_LOST__KIND => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::PACKET_LOST__KIND], entries, variants)
                }
                id::NOMINAL_COUNTERS_CONGESTION__SOURCE => {
                    let offset = *entry;
                    let variants = <CongestionSource as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::CONGESTION__SOURCE], entries, variants)
                }
                id::NOMINAL_COUNTERS_ACK_RANGE_RECEIVED__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::ACK_RANGE_RECEIVED__PACKET], entries, variants)
                }
                id::NOMINAL_COUNTERS_ACK_RANGE_SENT__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::ACK_RANGE_SENT__PACKET], entries, variants)
                }
                id::NOMINAL_COUNTERS_PACKET_DROPPED__REASON => {
                    let offset = *entry;
                    let variants = <PacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::PACKET_DROPPED__REASON], entries, variants)
                }
                id::NOMINAL_COUNTERS_KEY_UPDATE__KEY_TYPE => {
                    let offset = *entry;
                    let variants = <KeyType as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::KEY_UPDATE__KEY_TYPE], entries, variants)
                }
                id::NOMINAL_COUNTERS_KEY_UPDATE__CIPHER_SUITE => {
                    let offset = *entry;
                    let variants = <CipherSuite as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::KEY_UPDATE__CIPHER_SUITE], entries, variants)
                }
                id::NOMINAL_COUNTERS_KEY_SPACE_DISCARDED__SPACE => {
                    let offset = *entry;
                    let variants = <KeySpace as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::KEY_SPACE_DISCARDED__SPACE], entries, variants)
                }
                id::NOMINAL_COUNTERS_DUPLICATE_PACKET__KIND => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::DUPLICATE_PACKET__KIND], entries, variants)
                }
                id::NOMINAL_COUNTERS_DUPLICATE_PACKET__ERROR => {
                    let offset = *entry;
                    let variants = <DuplicatePacketError as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::DUPLICATE_PACKET__ERROR], entries, variants)
                }
                id::NOMINAL_COUNTERS_DATAGRAM_DROPPED__REASON => {
                    let offset = *entry;
                    let variants = <DatagramDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::DATAGRAM_DROPPED__REASON], entries, variants)
                }
                id::NOMINAL_COUNTERS_ECN_STATE_CHANGED__STATE => {
                    let offset = *entry;
                    let variants = <EcnState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::ECN_STATE_CHANGED__STATE], entries, variants)
                }
                id::NOMINAL_COUNTERS_CONNECTION_MIGRATION_DENIED__REASON => {
                    let offset = *entry;
                    let variants = <MigrationDenyReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::CONNECTION_MIGRATION_DENIED__REASON],
                        entries,
                        variants,
                    )
                }
                id::NOMINAL_COUNTERS_HANDSHAKE_STATUS_UPDATED__STATUS => {
                    let offset = *entry;
                    let variants = <HandshakeStatus as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::HANDSHAKE_STATUS_UPDATED__STATUS],
                        entries,
                        variants,
                    )
                }
                id::NOMINAL_COUNTERS_PATH_CHALLENGE_UPDATED__STATUS => {
                    let offset = *entry;
                    let variants = <PathChallengeStatus as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::PATH_CHALLENGE_UPDATED__STATUS], entries, variants)
                }
                id::NOMINAL_COUNTERS_MTU_UPDATED__CAUSE => {
                    let offset = *entry;
                    let variants = <MtuUpdatedCause as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::MTU_UPDATED__CAUSE], entries, variants)
                }
                id::NOMINAL_COUNTERS_MTU_PROBING_COMPLETE_RECEIVED__PACKET => {
                    let offset = *entry;
                    let variants = <PacketHeader as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::MTU_PROBING_COMPLETE_RECEIVED__PACKET],
                        entries,
                        variants,
                    )
                }
                id::NOMINAL_COUNTERS_SLOW_START_EXITED__CAUSE => {
                    let offset = *entry;
                    let variants = <SlowStartExitCause as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::SLOW_START_EXITED__CAUSE], entries, variants)
                }
                id::NOMINAL_COUNTERS_BBR_STATE_CHANGED__STATE => {
                    let offset = *entry;
                    let variants = <BbrState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::BBR_STATE_CHANGED__STATE], entries, variants)
                }
                id::NOMINAL_COUNTERS_DC_STATE_CHANGED__STATE => {
                    let offset = *entry;
                    let variants = <DcState as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::DC_STATE_CHANGED__STATE], entries, variants)
                }
                id::NOMINAL_COUNTERS_CONNECTION_CLOSED__ERROR => {
                    let offset = *entry;
                    let variants = <crate::connection::Error as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[id::CONNECTION_CLOSED__ERROR], entries, variants)
                }
                id::NOMINAL_COUNTERS_ENDPOINT_DATAGRAM_DROPPED__REASON => {
                    let offset = *entry;
                    let variants = <DatagramDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::ENDPOINT_DATAGRAM_DROPPED__REASON],
                        entries,
                        variants,
                    )
                }
                id::NOMINAL_COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR => {
                    let offset = *entry;
                    let variants = <crate::connection::Error as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (
                        &INFO[id::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR],
                        entries,
                        variants,
                    )
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_nominal<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
        let info = &INFO[info];
        let idx = self.nominal_counter_offsets[id] + value.variant_idx();
        let counter = &self.nominal_counters[idx];
        counter.record(info, value.as_variant(), 1usize);
    }
    #[doc = r" Returns all of the registered measures"]
    #[inline]
    pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
        self.measures
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                id::MEASURES_PACKET_SENT__BYTES => (&INFO[id::PACKET_SENT__BYTES], entry),
                id::MEASURES_PACKET_LOST__BYTES => (&INFO[id::PACKET_LOST__BYTES], entry),
                id::MEASURES_RECOVERY_METRICS__MIN_RTT => {
                    (&INFO[id::RECOVERY_METRICS__MIN_RTT], entry)
                }
                id::MEASURES_RECOVERY_METRICS__SMOOTHED_RTT => {
                    (&INFO[id::RECOVERY_METRICS__SMOOTHED_RTT], entry)
                }
                id::MEASURES_RECOVERY_METRICS__LATEST_RTT => {
                    (&INFO[id::RECOVERY_METRICS__LATEST_RTT], entry)
                }
                id::MEASURES_RECOVERY_METRICS__RTT_VARIANCE => {
                    (&INFO[id::RECOVERY_METRICS__RTT_VARIANCE], entry)
                }
                id::MEASURES_RECOVERY_METRICS__MAX_ACK_DELAY => {
                    (&INFO[id::RECOVERY_METRICS__MAX_ACK_DELAY], entry)
                }
                id::MEASURES_RECOVERY_METRICS__PTO_COUNT => {
                    (&INFO[id::RECOVERY_METRICS__PTO_COUNT], entry)
                }
                id::MEASURES_RECOVERY_METRICS__CONGESTION_WINDOW => {
                    (&INFO[id::RECOVERY_METRICS__CONGESTION_WINDOW], entry)
                }
                id::MEASURES_RECOVERY_METRICS__BYTES_IN_FLIGHT => {
                    (&INFO[id::RECOVERY_METRICS__BYTES_IN_FLIGHT], entry)
                }
                id::MEASURES_DATAGRAM_SENT__BYTES => (&INFO[id::DATAGRAM_SENT__BYTES], entry),
                id::MEASURES_DATAGRAM_SENT__GSO_OFFSET => {
                    (&INFO[id::DATAGRAM_SENT__GSO_OFFSET], entry)
                }
                id::MEASURES_DATAGRAM_RECEIVED__BYTES => {
                    (&INFO[id::DATAGRAM_RECEIVED__BYTES], entry)
                }
                id::MEASURES_DATAGRAM_DROPPED__BYTES => (&INFO[id::DATAGRAM_DROPPED__BYTES], entry),
                id::MEASURES_RX_STREAM_PROGRESS__BYTES => {
                    (&INFO[id::RX_STREAM_PROGRESS__BYTES], entry)
                }
                id::MEASURES_TX_STREAM_PROGRESS__BYTES => {
                    (&INFO[id::TX_STREAM_PROGRESS__BYTES], entry)
                }
                id::MEASURES_MTU_UPDATED__MTU => (&INFO[id::MTU_UPDATED__MTU], entry),
                id::MEASURES_MTU_PROBING_COMPLETE_RECEIVED__MTU => {
                    (&INFO[id::MTU_PROBING_COMPLETE_RECEIVED__MTU], entry)
                }
                id::MEASURES_SLOW_START_EXITED__CONGESTION_WINDOW => {
                    (&INFO[id::SLOW_START_EXITED__CONGESTION_WINDOW], entry)
                }
                id::MEASURES_PACING_RATE_UPDATED__BYTES_PER_SECOND => {
                    (&INFO[id::PACING_RATE_UPDATED__BYTES_PER_SECOND], entry)
                }
                id::MEASURES_PACING_RATE_UPDATED__BURST_SIZE => {
                    (&INFO[id::PACING_RATE_UPDATED__BURST_SIZE], entry)
                }
                id::MEASURES_PACING_RATE_UPDATED__PACING_GAIN => {
                    (&INFO[id::PACING_RATE_UPDATED__PACING_GAIN], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES => {
                    (&INFO[id::ENDPOINT_DATAGRAM_SENT__BYTES], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL => {
                    (&INFO[id::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_SENT__GSO_OFFSET => {
                    (&INFO[id::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES => {
                    (&INFO[id::ENDPOINT_DATAGRAM_RECEIVED__BYTES], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL => {
                    (&INFO[id::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES => {
                    (&INFO[id::ENDPOINT_DATAGRAM_DROPPED__BYTES], entry)
                }
                id::MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL => {
                    (&INFO[id::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL], entry)
                }
                id::MEASURES_PLATFORM_TX__PACKETS => (&INFO[id::PLATFORM_TX__PACKETS], entry),
                id::MEASURES_PLATFORM_TX__SYSCALLS => (&INFO[id::PLATFORM_TX__SYSCALLS], entry),
                id::MEASURES_PLATFORM_TX__SYSCALLS__BLOCKED => {
                    (&INFO[id::PLATFORM_TX__SYSCALLS__BLOCKED], entry)
                }
                id::MEASURES_PLATFORM_TX__ERRORS => (&INFO[id::PLATFORM_TX__ERRORS], entry),
                id::MEASURES_PLATFORM_TX__ERRORS__DROPPED => {
                    (&INFO[id::PLATFORM_TX__ERRORS__DROPPED], entry)
                }
                id::MEASURES_PLATFORM_RX__PACKETS => (&INFO[id::PLATFORM_RX__PACKETS], entry),
                id::MEASURES_PLATFORM_RX__SYSCALLS => (&INFO[id::PLATFORM_RX__SYSCALLS], entry),
                id::MEASURES_PLATFORM_RX__SYSCALLS__BLOCKED => {
                    (&INFO[id::PLATFORM_RX__SYSCALLS__BLOCKED], entry)
                }
                id::MEASURES_PLATFORM_RX__ERRORS => (&INFO[id::PLATFORM_RX__ERRORS], entry),
                id::MEASURES_PLATFORM_RX__ERRORS__DROPPED => {
                    (&INFO[id::PLATFORM_RX__ERRORS__DROPPED], entry)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn measure<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let measure = &self.measures[id];
        measure.record(info, value);
    }
    #[doc = r" Returns all of the registered gauges"]
    #[inline]
    pub fn gauges(&self) -> impl Iterator<Item = (&'static Info, &R::Gauge)> + '_ {
        core::iter::empty()
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn gauge<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let gauge = &self.gauges[id];
        gauge.record(info, value);
    }
    #[doc = r" Returns all of the registered timers"]
    #[inline]
    pub fn timers(&self) -> impl Iterator<Item = (&'static Info, &R::Timer)> + '_ {
        self.timers
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                id::TIMERS_KEY_SPACE_DISCARDED__INITIAL__LATENCY => {
                    (&INFO[id::KEY_SPACE_DISCARDED__INITIAL__LATENCY], entry)
                }
                id::TIMERS_KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY => {
                    (&INFO[id::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY], entry)
                }
                id::TIMERS_KEY_SPACE_DISCARDED__ONE_RTT__LATENCY => {
                    (&INFO[id::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY], entry)
                }
                id::TIMERS_TRANSPORT_PARAMETERS_RECEIVED__LATENCY => {
                    (&INFO[id::TRANSPORT_PARAMETERS_RECEIVED__LATENCY], entry)
                }
                id::TIMERS_HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY => (
                    &INFO[id::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY],
                    entry,
                ),
                id::TIMERS_HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY => (
                    &INFO[id::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY],
                    entry,
                ),
                id::TIMERS_HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY => (
                    &INFO[id::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY],
                    entry,
                ),
                id::TIMERS_TLS_CLIENT_HELLO__LATENCY => {
                    (&INFO[id::TLS_CLIENT_HELLO__LATENCY], entry)
                }
                id::TIMERS_TLS_SERVER_HELLO__LATENCY => {
                    (&INFO[id::TLS_SERVER_HELLO__LATENCY], entry)
                }
                id::TIMERS_DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY => (
                    &INFO[id::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY],
                    entry,
                ),
                id::TIMERS_DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY => (
                    &INFO[id::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY],
                    entry,
                ),
                id::TIMERS_DC_STATE_CHANGED__PATH_SECRETS__LATENCY => {
                    (&INFO[id::DC_STATE_CHANGED__PATH_SECRETS__LATENCY], entry)
                }
                id::TIMERS_DC_STATE_CHANGED__COMPLETE__LATENCY => {
                    (&INFO[id::DC_STATE_CHANGED__COMPLETE__LATENCY], entry)
                }
                id::TIMERS_CONNECTION_CLOSED__LATENCY => {
                    (&INFO[id::CONNECTION_CLOSED__LATENCY], entry)
                }
                id::TIMERS_PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION => (
                    &INFO[id::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION],
                    entry,
                ),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time(&self, info: usize, id: usize, value: core::time::Duration) {
        let info = &INFO[info];
        let timer = &self.timers[id];
        timer.record(info, value);
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time_nominal<T: AsVariant>(
        &self,
        info: usize,
        id: usize,
        value: &T,
        duration: core::time::Duration,
    ) {
        let info = &INFO[info];
        let idx = self.nominal_timer_offsets[id] + value.variant_idx();
        let counter = &self.nominal_timers[idx];
        counter.record(info, value.as_variant(), duration);
    }
}
impl<R: Registry> event::Subscriber for Subscriber<R> {
    type ConnectionContext = ConnectionContext;
    fn create_connection_context(
        &mut self,
        meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Self::ConnectionContext {
            start_time: meta.timestamp,
        }
    }
    #[inline]
    fn on_application_protocol_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationProtocolInformation,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::APPLICATION_PROTOCOL_INFORMATION,
            id::COUNTERS_APPLICATION_PROTOCOL_INFORMATION,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_server_name_information(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ServerNameInformation,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::SERVER_NAME_INFORMATION,
            id::COUNTERS_SERVER_NAME_INFORMATION,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_key_exchange_group(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeyExchangeGroup,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::KEY_EXCHANGE_GROUP,
            id::COUNTERS_KEY_EXCHANGE_GROUP,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_skipped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSkipped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PACKET_SKIPPED, id::COUNTERS_PACKET_SKIPPED, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PACKET_SENT, id::COUNTERS_PACKET_SENT, 1usize);
        self.count_nominal(
            id::PACKET_SENT__KIND,
            id::NOMINAL_COUNTERS_PACKET_SENT__KIND,
            &event.packet_header,
        );
        self.count(
            id::PACKET_SENT__BYTES__TOTAL,
            id::COUNTERS_PACKET_SENT__BYTES__TOTAL,
            event.packet_len,
        );
        self.measure(
            id::PACKET_SENT__BYTES,
            id::MEASURES_PACKET_SENT__BYTES,
            event.packet_len,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PACKET_RECEIVED, id::COUNTERS_PACKET_RECEIVED, 1usize);
        self.count_nominal(
            id::PACKET_RECEIVED__KIND,
            id::NOMINAL_COUNTERS_PACKET_RECEIVED__KIND,
            &event.packet_header,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_active_path_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ActivePathUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ACTIVE_PATH_UPDATED,
            id::COUNTERS_ACTIVE_PATH_UPDATED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_path_created(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathCreated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PATH_CREATED, id::COUNTERS_PATH_CREATED, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_frame_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::FRAME_SENT, id::COUNTERS_FRAME_SENT, 1usize);
        self.count_nominal(
            id::FRAME_SENT__PACKET,
            id::NOMINAL_COUNTERS_FRAME_SENT__PACKET,
            &event.packet_header,
        );
        self.count_nominal(
            id::FRAME_SENT__FRAME,
            id::NOMINAL_COUNTERS_FRAME_SENT__FRAME,
            &event.frame,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::FrameReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::FRAME_RECEIVED, id::COUNTERS_FRAME_RECEIVED, 1usize);
        self.count_nominal(
            id::FRAME_RECEIVED__PACKET,
            id::NOMINAL_COUNTERS_FRAME_RECEIVED__PACKET,
            &event.packet_header,
        );
        self.count_nominal(
            id::FRAME_RECEIVED__FRAME,
            id::NOMINAL_COUNTERS_FRAME_RECEIVED__FRAME,
            &event.frame,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_close_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionCloseFrameReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::CONNECTION_CLOSE_FRAME_RECEIVED,
            id::COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED,
            1usize,
        );
        self.count_nominal(
            id::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET,
            id::NOMINAL_COUNTERS_CONNECTION_CLOSE_FRAME_RECEIVED__PACKET,
            &event.packet_header,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_lost(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketLost,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PACKET_LOST, id::COUNTERS_PACKET_LOST, 1usize);
        self.count_nominal(
            id::PACKET_LOST__KIND,
            id::NOMINAL_COUNTERS_PACKET_LOST__KIND,
            &event.packet_header,
        );
        self.count(
            id::PACKET_LOST__BYTES__TOTAL,
            id::COUNTERS_PACKET_LOST__BYTES__TOTAL,
            event.bytes_lost,
        );
        self.measure(
            id::PACKET_LOST__BYTES,
            id::MEASURES_PACKET_LOST__BYTES,
            event.bytes_lost,
        );
        self.count_bool(
            id::PACKET_LOST__IS_MTU_PROBE,
            id::BOOL_COUNTERS_PACKET_LOST__IS_MTU_PROBE,
            event.is_mtu_probe,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_recovery_metrics(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RecoveryMetrics,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::RECOVERY_METRICS, id::COUNTERS_RECOVERY_METRICS, 1usize);
        self.measure(
            id::RECOVERY_METRICS__MIN_RTT,
            id::MEASURES_RECOVERY_METRICS__MIN_RTT,
            event.min_rtt,
        );
        self.measure(
            id::RECOVERY_METRICS__SMOOTHED_RTT,
            id::MEASURES_RECOVERY_METRICS__SMOOTHED_RTT,
            event.smoothed_rtt,
        );
        self.measure(
            id::RECOVERY_METRICS__LATEST_RTT,
            id::MEASURES_RECOVERY_METRICS__LATEST_RTT,
            event.latest_rtt,
        );
        self.measure(
            id::RECOVERY_METRICS__RTT_VARIANCE,
            id::MEASURES_RECOVERY_METRICS__RTT_VARIANCE,
            event.rtt_variance,
        );
        self.measure(
            id::RECOVERY_METRICS__MAX_ACK_DELAY,
            id::MEASURES_RECOVERY_METRICS__MAX_ACK_DELAY,
            event.max_ack_delay,
        );
        self.measure(
            id::RECOVERY_METRICS__PTO_COUNT,
            id::MEASURES_RECOVERY_METRICS__PTO_COUNT,
            event.pto_count,
        );
        self.measure(
            id::RECOVERY_METRICS__CONGESTION_WINDOW,
            id::MEASURES_RECOVERY_METRICS__CONGESTION_WINDOW,
            event.congestion_window,
        );
        self.measure(
            id::RECOVERY_METRICS__BYTES_IN_FLIGHT,
            id::MEASURES_RECOVERY_METRICS__BYTES_IN_FLIGHT,
            event.bytes_in_flight,
        );
        self.count_bool(
            id::RECOVERY_METRICS__CONGESTION_LIMITED,
            id::BOOL_COUNTERS_RECOVERY_METRICS__CONGESTION_LIMITED,
            event.congestion_limited,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_congestion(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::Congestion,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::CONGESTION, id::COUNTERS_CONGESTION, 1usize);
        self.count_nominal(
            id::CONGESTION__SOURCE,
            id::NOMINAL_COUNTERS_CONGESTION__SOURCE,
            &event.source,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_rx_ack_range_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxAckRangeDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::RX_ACK_RANGE_DROPPED,
            id::COUNTERS_RX_ACK_RANGE_DROPPED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ack_range_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ACK_RANGE_RECEIVED,
            id::COUNTERS_ACK_RANGE_RECEIVED,
            1usize,
        );
        self.count_nominal(
            id::ACK_RANGE_RECEIVED__PACKET,
            id::NOMINAL_COUNTERS_ACK_RANGE_RECEIVED__PACKET,
            &event.packet_header,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ack_range_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::AckRangeSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::ACK_RANGE_SENT, id::COUNTERS_ACK_RANGE_SENT, 1usize);
        self.count_nominal(
            id::ACK_RANGE_SENT__PACKET,
            id::NOMINAL_COUNTERS_ACK_RANGE_SENT__PACKET,
            &event.packet_header,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_packet_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PACKET_DROPPED, id::COUNTERS_PACKET_DROPPED, 1usize);
        self.count_nominal(
            id::PACKET_DROPPED__REASON,
            id::NOMINAL_COUNTERS_PACKET_DROPPED__REASON,
            &event.reason,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_key_update(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeyUpdate,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::KEY_UPDATE, id::COUNTERS_KEY_UPDATE, 1usize);
        self.count_nominal(
            id::KEY_UPDATE__KEY_TYPE,
            id::NOMINAL_COUNTERS_KEY_UPDATE__KEY_TYPE,
            &event.key_type,
        );
        self.count_nominal(
            id::KEY_UPDATE__CIPHER_SUITE,
            id::NOMINAL_COUNTERS_KEY_UPDATE__CIPHER_SUITE,
            &event.cipher_suite,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_key_space_discarded(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeySpaceDiscarded,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::KEY_SPACE_DISCARDED,
            id::COUNTERS_KEY_SPACE_DISCARDED,
            1usize,
        );
        {
            fn check(evt: &api::KeySpaceDiscarded) -> bool {
                matches!(evt.space, KeySpace::Initial { .. })
            }
            if check(event) {
                self.time(
                    id::KEY_SPACE_DISCARDED__INITIAL__LATENCY,
                    id::TIMERS_KEY_SPACE_DISCARDED__INITIAL__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::KeySpaceDiscarded) -> bool {
                matches!(evt.space, KeySpace::Handshake { .. })
            }
            if check(event) {
                self.time(
                    id::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY,
                    id::TIMERS_KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::KeySpaceDiscarded) -> bool {
                matches!(evt.space, KeySpace::OneRtt { .. })
            }
            if check(event) {
                self.time(
                    id::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY,
                    id::TIMERS_KEY_SPACE_DISCARDED__ONE_RTT__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        self.count_nominal(
            id::KEY_SPACE_DISCARDED__SPACE,
            id::NOMINAL_COUNTERS_KEY_SPACE_DISCARDED__SPACE,
            &event.space,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_started(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionStarted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::CONNECTION_STARTED,
            id::COUNTERS_CONNECTION_STARTED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_duplicate_packet(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DuplicatePacket,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::DUPLICATE_PACKET, id::COUNTERS_DUPLICATE_PACKET, 1usize);
        self.count_nominal(
            id::DUPLICATE_PACKET__KIND,
            id::NOMINAL_COUNTERS_DUPLICATE_PACKET__KIND,
            &event.packet_header,
        );
        self.count_nominal(
            id::DUPLICATE_PACKET__ERROR,
            id::NOMINAL_COUNTERS_DUPLICATE_PACKET__ERROR,
            &event.error,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_transport_parameters_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TransportParametersReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::TRANSPORT_PARAMETERS_RECEIVED,
            id::COUNTERS_TRANSPORT_PARAMETERS_RECEIVED,
            1usize,
        );
        self.time(
            id::TRANSPORT_PARAMETERS_RECEIVED__LATENCY,
            id::TIMERS_TRANSPORT_PARAMETERS_RECEIVED__LATENCY,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::DATAGRAM_SENT, id::COUNTERS_DATAGRAM_SENT, 1usize);
        self.count(
            id::DATAGRAM_SENT__BYTES__TOTAL,
            id::COUNTERS_DATAGRAM_SENT__BYTES__TOTAL,
            event.len,
        );
        self.measure(
            id::DATAGRAM_SENT__BYTES,
            id::MEASURES_DATAGRAM_SENT__BYTES,
            event.len,
        );
        self.measure(
            id::DATAGRAM_SENT__GSO_OFFSET,
            id::MEASURES_DATAGRAM_SENT__GSO_OFFSET,
            event.gso_offset,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::DATAGRAM_RECEIVED,
            id::COUNTERS_DATAGRAM_RECEIVED,
            1usize,
        );
        self.count(
            id::DATAGRAM_RECEIVED__BYTES__TOTAL,
            id::COUNTERS_DATAGRAM_RECEIVED__BYTES__TOTAL,
            event.len,
        );
        self.measure(
            id::DATAGRAM_RECEIVED__BYTES,
            id::MEASURES_DATAGRAM_RECEIVED__BYTES,
            event.len,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_datagram_dropped(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DatagramDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::DATAGRAM_DROPPED, id::COUNTERS_DATAGRAM_DROPPED, 1usize);
        self.count(
            id::DATAGRAM_DROPPED__BYTES__TOTAL,
            id::COUNTERS_DATAGRAM_DROPPED__BYTES__TOTAL,
            event.len,
        );
        self.measure(
            id::DATAGRAM_DROPPED__BYTES,
            id::MEASURES_DATAGRAM_DROPPED__BYTES,
            event.len,
        );
        self.count_nominal(
            id::DATAGRAM_DROPPED__REASON,
            id::NOMINAL_COUNTERS_DATAGRAM_DROPPED__REASON,
            &event.reason,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_handshake_remote_address_change_observed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::HandshakeRemoteAddressChangeObserved,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED,
            id::COUNTERS_HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_id_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionIdUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::CONNECTION_ID_UPDATED,
            id::COUNTERS_CONNECTION_ID_UPDATED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_ecn_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::EcnStateChanged,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ECN_STATE_CHANGED,
            id::COUNTERS_ECN_STATE_CHANGED,
            1usize,
        );
        self.count_nominal(
            id::ECN_STATE_CHANGED__STATE,
            id::NOMINAL_COUNTERS_ECN_STATE_CHANGED__STATE,
            &event.state,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_migration_denied(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionMigrationDenied,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::CONNECTION_MIGRATION_DENIED,
            id::COUNTERS_CONNECTION_MIGRATION_DENIED,
            1usize,
        );
        self.count_nominal(
            id::CONNECTION_MIGRATION_DENIED__REASON,
            id::NOMINAL_COUNTERS_CONNECTION_MIGRATION_DENIED__REASON,
            &event.reason,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::HandshakeStatusUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::HANDSHAKE_STATUS_UPDATED,
            id::COUNTERS_HANDSHAKE_STATUS_UPDATED,
            1usize,
        );
        {
            fn check(evt: &api::HandshakeStatusUpdated) -> bool {
                matches!(evt.status, HandshakeStatus::Complete { .. })
            }
            if check(event) {
                self.time(
                    id::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY,
                    id::TIMERS_HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::HandshakeStatusUpdated) -> bool {
                matches!(evt.status, HandshakeStatus::Confirmed { .. })
            }
            if check(event) {
                self.time(
                    id::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY,
                    id::TIMERS_HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::HandshakeStatusUpdated) -> bool {
                matches!(evt.status, HandshakeStatus::HandshakeDoneAcked { .. })
            }
            if check(event) {
                self.time(
                    id::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY,
                    id::TIMERS_HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        self.count_nominal(
            id::HANDSHAKE_STATUS_UPDATED__STATUS,
            id::NOMINAL_COUNTERS_HANDSHAKE_STATUS_UPDATED__STATUS,
            &event.status,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_exporter_ready(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsExporterReady,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::TLS_EXPORTER_READY,
            id::COUNTERS_TLS_EXPORTER_READY,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_handshake_failed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsHandshakeFailed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::TLS_HANDSHAKE_FAILED,
            id::COUNTERS_TLS_HANDSHAKE_FAILED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_path_challenge_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PathChallengeUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PATH_CHALLENGE_UPDATED,
            id::COUNTERS_PATH_CHALLENGE_UPDATED,
            1usize,
        );
        self.count_nominal(
            id::PATH_CHALLENGE_UPDATED__STATUS,
            id::NOMINAL_COUNTERS_PATH_CHALLENGE_UPDATED__STATUS,
            &event.path_challenge_status,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_client_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsClientHello,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::TLS_CLIENT_HELLO, id::COUNTERS_TLS_CLIENT_HELLO, 1usize);
        self.time(
            id::TLS_CLIENT_HELLO__LATENCY,
            id::TIMERS_TLS_CLIENT_HELLO__LATENCY,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tls_server_hello(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TlsServerHello,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::TLS_SERVER_HELLO, id::COUNTERS_TLS_SERVER_HELLO, 1usize);
        self.time(
            id::TLS_SERVER_HELLO__LATENCY,
            id::TIMERS_TLS_SERVER_HELLO__LATENCY,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_rx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::RxStreamProgress,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::RX_STREAM_PROGRESS,
            id::COUNTERS_RX_STREAM_PROGRESS,
            1usize,
        );
        self.count(
            id::RX_STREAM_PROGRESS__BYTES__TOTAL,
            id::COUNTERS_RX_STREAM_PROGRESS__BYTES__TOTAL,
            event.bytes,
        );
        self.measure(
            id::RX_STREAM_PROGRESS__BYTES,
            id::MEASURES_RX_STREAM_PROGRESS__BYTES,
            event.bytes,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_tx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::TxStreamProgress,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::TX_STREAM_PROGRESS,
            id::COUNTERS_TX_STREAM_PROGRESS,
            1usize,
        );
        self.count(
            id::TX_STREAM_PROGRESS__BYTES__TOTAL,
            id::COUNTERS_TX_STREAM_PROGRESS__BYTES__TOTAL,
            event.bytes,
        );
        self.measure(
            id::TX_STREAM_PROGRESS__BYTES,
            id::MEASURES_TX_STREAM_PROGRESS__BYTES,
            event.bytes,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_keep_alive_timer_expired(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::KeepAliveTimerExpired,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::KEEP_ALIVE_TIMER_EXPIRED,
            id::COUNTERS_KEEP_ALIVE_TIMER_EXPIRED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::MtuUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::MTU_UPDATED, id::COUNTERS_MTU_UPDATED, 1usize);
        self.measure(
            id::MTU_UPDATED__MTU,
            id::MEASURES_MTU_UPDATED__MTU,
            event.mtu,
        );
        self.count_nominal(
            id::MTU_UPDATED__CAUSE,
            id::NOMINAL_COUNTERS_MTU_UPDATED__CAUSE,
            &event.cause,
        );
        self.count_bool(
            id::MTU_UPDATED__SEARCH_COMPLETE,
            id::BOOL_COUNTERS_MTU_UPDATED__SEARCH_COMPLETE,
            event.search_complete,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_mtu_probing_complete_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::MtuProbingCompleteReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::MTU_PROBING_COMPLETE_RECEIVED,
            id::COUNTERS_MTU_PROBING_COMPLETE_RECEIVED,
            1usize,
        );
        self.count_nominal(
            id::MTU_PROBING_COMPLETE_RECEIVED__PACKET,
            id::NOMINAL_COUNTERS_MTU_PROBING_COMPLETE_RECEIVED__PACKET,
            &event.packet_header,
        );
        self.measure(
            id::MTU_PROBING_COMPLETE_RECEIVED__MTU,
            id::MEASURES_MTU_PROBING_COMPLETE_RECEIVED__MTU,
            event.mtu,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_slow_start_exited(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::SlowStartExited,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::SLOW_START_EXITED,
            id::COUNTERS_SLOW_START_EXITED,
            1usize,
        );
        self.count_nominal(
            id::SLOW_START_EXITED__CAUSE,
            id::NOMINAL_COUNTERS_SLOW_START_EXITED__CAUSE,
            &event.cause,
        );
        self.time_nominal(
            id::SLOW_START_EXITED__LATENCY,
            id::NOMINAL_TIMERS_SLOW_START_EXITED__LATENCY,
            &event.cause,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(
            id::SLOW_START_EXITED__CONGESTION_WINDOW,
            id::MEASURES_SLOW_START_EXITED__CONGESTION_WINDOW,
            event.congestion_window,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_delivery_rate_sampled(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DeliveryRateSampled,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::DELIVERY_RATE_SAMPLED,
            id::COUNTERS_DELIVERY_RATE_SAMPLED,
            1usize,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::PacingRateUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PACING_RATE_UPDATED,
            id::COUNTERS_PACING_RATE_UPDATED,
            1usize,
        );
        self.measure(
            id::PACING_RATE_UPDATED__BYTES_PER_SECOND,
            id::MEASURES_PACING_RATE_UPDATED__BYTES_PER_SECOND,
            event.bytes_per_second,
        );
        self.measure(
            id::PACING_RATE_UPDATED__BURST_SIZE,
            id::MEASURES_PACING_RATE_UPDATED__BURST_SIZE,
            event.burst_size,
        );
        self.measure(
            id::PACING_RATE_UPDATED__PACING_GAIN,
            id::MEASURES_PACING_RATE_UPDATED__PACING_GAIN,
            event.pacing_gain,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_bbr_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::BbrStateChanged,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::BBR_STATE_CHANGED,
            id::COUNTERS_BBR_STATE_CHANGED,
            1usize,
        );
        self.count_nominal(
            id::BBR_STATE_CHANGED__STATE,
            id::NOMINAL_COUNTERS_BBR_STATE_CHANGED__STATE,
            &event.state,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DcStateChanged,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::DC_STATE_CHANGED, id::COUNTERS_DC_STATE_CHANGED, 1usize);
        {
            fn check(evt: &api::DcStateChanged) -> bool {
                matches!(evt.state, DcState::VersionNegotiated { .. })
            }
            if check(event) {
                self.time(
                    id::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY,
                    id::TIMERS_DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::DcStateChanged) -> bool {
                matches!(evt.state, DcState::VersionNegotiated { .. })
            }
            if check(event) {
                self.time(
                    id::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY,
                    id::TIMERS_DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::DcStateChanged) -> bool {
                matches!(evt.state, DcState::PathSecretsReady { .. })
            }
            if check(event) {
                self.time(
                    id::DC_STATE_CHANGED__PATH_SECRETS__LATENCY,
                    id::TIMERS_DC_STATE_CHANGED__PATH_SECRETS__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        {
            fn check(evt: &api::DcStateChanged) -> bool {
                matches!(evt.state, DcState::Complete { .. })
            }
            if check(event) {
                self.time(
                    id::DC_STATE_CHANGED__COMPLETE__LATENCY,
                    id::TIMERS_DC_STATE_CHANGED__COMPLETE__LATENCY,
                    meta.timestamp.saturating_duration_since(context.start_time),
                );
            }
        }
        self.count_nominal(
            id::DC_STATE_CHANGED__STATE,
            id::NOMINAL_COUNTERS_DC_STATE_CHANGED__STATE,
            &event.state,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_dc_path_created(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::DcPathCreated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::DC_PATH_CREATED, id::COUNTERS_DC_PATH_CREATED, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionClosed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::CONNECTION_CLOSED,
            id::COUNTERS_CONNECTION_CLOSED,
            1usize,
        );
        self.time(
            id::CONNECTION_CLOSED__LATENCY,
            id::TIMERS_CONNECTION_CLOSED__LATENCY,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.count_nominal(
            id::CONNECTION_CLOSED__ERROR,
            id::NOMINAL_COUNTERS_CONNECTION_CLOSED__ERROR,
            &event.error,
        );
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_version_information(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::VersionInformation,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::VERSION_INFORMATION,
            id::COUNTERS_VERSION_INFORMATION,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_PACKET_SENT,
            id::COUNTERS_ENDPOINT_PACKET_SENT,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_packet_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_PACKET_RECEIVED,
            id::COUNTERS_ENDPOINT_PACKET_RECEIVED,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_sent(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_DATAGRAM_SENT,
            id::COUNTERS_ENDPOINT_DATAGRAM_SENT,
            1usize,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_SENT__BYTES,
            id::MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES,
            event.len,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL,
            id::MEASURES_ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL,
            event.len,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET,
            id::MEASURES_ENDPOINT_DATAGRAM_SENT__GSO_OFFSET,
            event.gso_offset,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_received(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_DATAGRAM_RECEIVED,
            id::COUNTERS_ENDPOINT_DATAGRAM_RECEIVED,
            1usize,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_RECEIVED__BYTES,
            id::MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES,
            event.len,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL,
            id::MEASURES_ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL,
            event.len,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_datagram_dropped(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointDatagramDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_DATAGRAM_DROPPED,
            id::COUNTERS_ENDPOINT_DATAGRAM_DROPPED,
            1usize,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_DROPPED__BYTES,
            id::MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES,
            event.len,
        );
        self.measure(
            id::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL,
            id::MEASURES_ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL,
            event.len,
        );
        self.count_nominal(
            id::ENDPOINT_DATAGRAM_DROPPED__REASON,
            id::NOMINAL_COUNTERS_ENDPOINT_DATAGRAM_DROPPED__REASON,
            &event.reason,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_connection_attempt_failed(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointConnectionAttemptFailed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_CONNECTION_ATTEMPT_FAILED,
            id::COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED,
            1usize,
        );
        self.count_nominal(
            id::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR,
            id::NOMINAL_COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR,
            &event.error,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_endpoint_connection_attempt_deduplicated(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::EndpointConnectionAttemptDeduplicated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED,
            id::COUNTERS_ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTx) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PLATFORM_TX, id::COUNTERS_PLATFORM_TX, 1usize);
        self.count(
            id::PLATFORM_TX__PACKETS__TOTAL,
            id::COUNTERS_PLATFORM_TX__PACKETS__TOTAL,
            event.count,
        );
        self.measure(
            id::PLATFORM_TX__PACKETS,
            id::MEASURES_PLATFORM_TX__PACKETS,
            event.count,
        );
        self.count(
            id::PLATFORM_TX__SYSCALLS__TOTAL,
            id::COUNTERS_PLATFORM_TX__SYSCALLS__TOTAL,
            event.syscalls,
        );
        self.measure(
            id::PLATFORM_TX__SYSCALLS,
            id::MEASURES_PLATFORM_TX__SYSCALLS,
            event.syscalls,
        );
        self.count(
            id::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL,
            id::COUNTERS_PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL,
            event.blocked_syscalls,
        );
        self.measure(
            id::PLATFORM_TX__SYSCALLS__BLOCKED,
            id::MEASURES_PLATFORM_TX__SYSCALLS__BLOCKED,
            event.blocked_syscalls,
        );
        self.count(
            id::PLATFORM_TX__ERRORS__TOTAL,
            id::COUNTERS_PLATFORM_TX__ERRORS__TOTAL,
            event.total_errors,
        );
        self.measure(
            id::PLATFORM_TX__ERRORS,
            id::MEASURES_PLATFORM_TX__ERRORS,
            event.total_errors,
        );
        self.count(
            id::PLATFORM_TX__ERRORS__DROPPED__TOTAL,
            id::COUNTERS_PLATFORM_TX__ERRORS__DROPPED__TOTAL,
            event.dropped_errors,
        );
        self.measure(
            id::PLATFORM_TX__ERRORS__DROPPED,
            id::MEASURES_PLATFORM_TX__ERRORS__DROPPED,
            event.dropped_errors,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_tx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformTxError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_TX_ERROR,
            id::COUNTERS_PLATFORM_TX_ERROR,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRx) {
        #[allow(unused_imports)]
        use api::*;
        self.count(id::PLATFORM_RX, id::COUNTERS_PLATFORM_RX, 1usize);
        self.count(
            id::PLATFORM_RX__PACKETS__TOTAL,
            id::COUNTERS_PLATFORM_RX__PACKETS__TOTAL,
            event.count,
        );
        self.measure(
            id::PLATFORM_RX__PACKETS,
            id::MEASURES_PLATFORM_RX__PACKETS,
            event.count,
        );
        self.count(
            id::PLATFORM_RX__SYSCALLS__TOTAL,
            id::COUNTERS_PLATFORM_RX__SYSCALLS__TOTAL,
            event.syscalls,
        );
        self.measure(
            id::PLATFORM_RX__SYSCALLS,
            id::MEASURES_PLATFORM_RX__SYSCALLS,
            event.syscalls,
        );
        self.count(
            id::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL,
            id::COUNTERS_PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL,
            event.blocked_syscalls,
        );
        self.measure(
            id::PLATFORM_RX__SYSCALLS__BLOCKED,
            id::MEASURES_PLATFORM_RX__SYSCALLS__BLOCKED,
            event.blocked_syscalls,
        );
        self.count(
            id::PLATFORM_RX__ERRORS__TOTAL,
            id::COUNTERS_PLATFORM_RX__ERRORS__TOTAL,
            event.total_errors,
        );
        self.measure(
            id::PLATFORM_RX__ERRORS,
            id::MEASURES_PLATFORM_RX__ERRORS,
            event.total_errors,
        );
        self.count(
            id::PLATFORM_RX__ERRORS__DROPPED__TOTAL,
            id::COUNTERS_PLATFORM_RX__ERRORS__DROPPED__TOTAL,
            event.dropped_errors,
        );
        self.measure(
            id::PLATFORM_RX__ERRORS__DROPPED,
            id::MEASURES_PLATFORM_RX__ERRORS__DROPPED,
            event.dropped_errors,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_rx_error(&mut self, meta: &api::EndpointMeta, event: &api::PlatformRxError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_RX_ERROR,
            id::COUNTERS_PLATFORM_RX_ERROR,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_feature_configured(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformFeatureConfigured,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_FEATURE_CONFIGURED,
            id::COUNTERS_PLATFORM_FEATURE_CONFIGURED,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_wakeup(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopWakeup,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_EVENT_LOOP_WAKEUP,
            id::COUNTERS_PLATFORM_EVENT_LOOP_WAKEUP,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_sleep(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopSleep,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_EVENT_LOOP_SLEEP,
            id::COUNTERS_PLATFORM_EVENT_LOOP_SLEEP,
            1usize,
        );
        self.time(
            id::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION,
            id::TIMERS_PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION,
            event.processing_duration,
        );
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_platform_event_loop_started(
        &mut self,
        meta: &api::EndpointMeta,
        event: &api::PlatformEventLoopStarted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(
            id::PLATFORM_EVENT_LOOP_STARTED,
            id::COUNTERS_PLATFORM_EVENT_LOOP_STARTED,
            1usize,
        );
        let _ = event;
        let _ = meta;
    }
}
