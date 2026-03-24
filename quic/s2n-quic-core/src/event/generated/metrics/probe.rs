// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

use crate::{
    event::metrics::aggregate::{
        self, info, BoolRecorder, Info, NominalRecorder, Recorder as MetricRecorder,
    },
    probe::define,
};
mod id {
    #[allow(non_camel_case_types)]
    #[allow(clippy::upper_case_acronyms)]
    enum InfoId {
        APPLICATION_PROTOCOL_INFORMATION,
        SERVER_NAME_INFORMATION,
        KEY_EXCHANGE_GROUP,
        PACKET_SKIPPED,
        PACKET_SENT,
        PACKET_SENT__KIND,
        PACKET_SENT__BYTES__TOTAL,
        PACKET_SENT__BYTES,
        PACKET_SENT__TRANSMISSION_MODE,
        PACKET_RECEIVED,
        PACKET_RECEIVED__KIND,
        PACKET_RECEIVED__BYTES__TOTAL,
        PACKET_RECEIVED__BYTES,
        ACTIVE_PATH_UPDATED,
        PATH_CREATED,
        FRAME_SENT,
        FRAME_SENT__PACKET,
        FRAME_SENT__FRAME,
        FRAME_RECEIVED,
        FRAME_RECEIVED__PACKET,
        FRAME_RECEIVED__FRAME,
        CONNECTION_CLOSE_FRAME_RECEIVED,
        CONNECTION_CLOSE_FRAME_RECEIVED__PACKET,
        PACKET_LOST,
        PACKET_LOST__KIND,
        PACKET_LOST__BYTES__TOTAL,
        PACKET_LOST__BYTES,
        PACKET_LOST__IS_MTU_PROBE,
        RECOVERY_METRICS,
        RECOVERY_METRICS__MIN_RTT,
        RECOVERY_METRICS__SMOOTHED_RTT,
        RECOVERY_METRICS__LATEST_RTT,
        RECOVERY_METRICS__RTT_VARIANCE,
        RECOVERY_METRICS__MAX_ACK_DELAY,
        RECOVERY_METRICS__PTO_COUNT,
        RECOVERY_METRICS__CONGESTION_WINDOW,
        RECOVERY_METRICS__BYTES_IN_FLIGHT,
        RECOVERY_METRICS__CONGESTION_LIMITED,
        CONGESTION,
        CONGESTION__SOURCE,
        RX_ACK_RANGE_DROPPED,
        ACK_RANGE_RECEIVED,
        ACK_RANGE_RECEIVED__PACKET,
        ACK_RANGE_SENT,
        ACK_RANGE_SENT__PACKET,
        PACKET_DROPPED,
        PACKET_DROPPED__REASON,
        KEY_UPDATE,
        KEY_UPDATE__KEY_TYPE,
        KEY_UPDATE__CIPHER_SUITE,
        KEY_SPACE_DISCARDED,
        KEY_SPACE_DISCARDED__INITIAL__LATENCY,
        KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY,
        KEY_SPACE_DISCARDED__ONE_RTT__LATENCY,
        KEY_SPACE_DISCARDED__SPACE,
        CONNECTION_STARTED,
        DUPLICATE_PACKET,
        DUPLICATE_PACKET__KIND,
        DUPLICATE_PACKET__ERROR,
        TRANSPORT_PARAMETERS_RECEIVED,
        TRANSPORT_PARAMETERS_RECEIVED__LATENCY,
        DATAGRAM_SENT,
        DATAGRAM_SENT__BYTES__TOTAL,
        DATAGRAM_SENT__BYTES,
        DATAGRAM_SENT__GSO_OFFSET,
        DATAGRAM_RECEIVED,
        DATAGRAM_RECEIVED__BYTES__TOTAL,
        DATAGRAM_RECEIVED__BYTES,
        DATAGRAM_DROPPED,
        DATAGRAM_DROPPED__BYTES__TOTAL,
        DATAGRAM_DROPPED__BYTES,
        DATAGRAM_DROPPED__REASON,
        HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED,
        CONNECTION_ID_UPDATED,
        ECN_STATE_CHANGED,
        ECN_STATE_CHANGED__STATE,
        CONNECTION_MIGRATION_DENIED,
        CONNECTION_MIGRATION_DENIED__REASON,
        HANDSHAKE_STATUS_UPDATED,
        HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY,
        HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY,
        HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY,
        HANDSHAKE_STATUS_UPDATED__STATUS,
        TLS_EXPORTER_READY,
        TLS_HANDSHAKE_FAILED,
        PATH_CHALLENGE_UPDATED,
        PATH_CHALLENGE_UPDATED__STATUS,
        TLS_CLIENT_HELLO,
        TLS_CLIENT_HELLO__LATENCY,
        TLS_SERVER_HELLO,
        TLS_SERVER_HELLO__LATENCY,
        RX_STREAM_PROGRESS,
        RX_STREAM_PROGRESS__BYTES__TOTAL,
        RX_STREAM_PROGRESS__BYTES,
        TX_STREAM_PROGRESS,
        TX_STREAM_PROGRESS__BYTES__TOTAL,
        TX_STREAM_PROGRESS__BYTES,
        KEEP_ALIVE_TIMER_EXPIRED,
        MTU_UPDATED,
        MTU_UPDATED__MTU,
        MTU_UPDATED__CAUSE,
        MTU_UPDATED__SEARCH_COMPLETE,
        MTU_PROBING_COMPLETE_RECEIVED,
        MTU_PROBING_COMPLETE_RECEIVED__PACKET,
        MTU_PROBING_COMPLETE_RECEIVED__MTU,
        SLOW_START_EXITED,
        SLOW_START_EXITED__CAUSE,
        SLOW_START_EXITED__LATENCY,
        SLOW_START_EXITED__CONGESTION_WINDOW,
        DELIVERY_RATE_SAMPLED,
        PACING_RATE_UPDATED,
        PACING_RATE_UPDATED__BYTES_PER_SECOND,
        PACING_RATE_UPDATED__BURST_SIZE,
        PACING_RATE_UPDATED__PACING_GAIN,
        BBR_STATE_CHANGED,
        BBR_STATE_CHANGED__STATE,
        DC_STATE_CHANGED,
        DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY,
        DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY,
        DC_STATE_CHANGED__PATH_SECRETS__LATENCY,
        DC_STATE_CHANGED__COMPLETE__LATENCY,
        DC_STATE_CHANGED__STATE,
        DC_PATH_CREATED,
        CONNECTION_CLOSED,
        CONNECTION_CLOSED__LATENCY,
        CONNECTION_CLOSED__ERROR,
        VERSION_INFORMATION,
        ENDPOINT_PACKET_SENT,
        ENDPOINT_PACKET_RECEIVED,
        ENDPOINT_DATAGRAM_SENT,
        ENDPOINT_DATAGRAM_SENT__BYTES,
        ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL,
        ENDPOINT_DATAGRAM_SENT__GSO_OFFSET,
        ENDPOINT_DATAGRAM_RECEIVED,
        ENDPOINT_DATAGRAM_RECEIVED__BYTES,
        ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL,
        ENDPOINT_DATAGRAM_DROPPED,
        ENDPOINT_DATAGRAM_DROPPED__BYTES,
        ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL,
        ENDPOINT_DATAGRAM_DROPPED__REASON,
        ENDPOINT_CONNECTION_ATTEMPT_FAILED,
        ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR,
        ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED,
        PLATFORM_TX,
        PLATFORM_TX__PACKETS__TOTAL,
        PLATFORM_TX__PACKETS,
        PLATFORM_TX__SYSCALLS__TOTAL,
        PLATFORM_TX__SYSCALLS,
        PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL,
        PLATFORM_TX__SYSCALLS__BLOCKED,
        PLATFORM_TX__ERRORS__TOTAL,
        PLATFORM_TX__ERRORS,
        PLATFORM_TX__ERRORS__DROPPED__TOTAL,
        PLATFORM_TX__ERRORS__DROPPED,
        PLATFORM_TX_ERROR,
        PLATFORM_RX,
        PLATFORM_RX__PACKETS__TOTAL,
        PLATFORM_RX__PACKETS,
        PLATFORM_RX__SYSCALLS__TOTAL,
        PLATFORM_RX__SYSCALLS,
        PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL,
        PLATFORM_RX__SYSCALLS__BLOCKED,
        PLATFORM_RX__ERRORS__TOTAL,
        PLATFORM_RX__ERRORS,
        PLATFORM_RX__ERRORS__DROPPED__TOTAL,
        PLATFORM_RX__ERRORS__DROPPED,
        PLATFORM_RX_ERROR,
        PLATFORM_FEATURE_CONFIGURED,
        PLATFORM_RX_SOCKET_STATS,
        PLATFORM_RX_SOCKET_STATS__PACKETS__TOTAL,
        PLATFORM_EVENT_LOOP_WAKEUP,
        PLATFORM_EVENT_LOOP_SLEEP,
        PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION,
        PLATFORM_EVENT_LOOP_STARTED,
    }
    pub const APPLICATION_PROTOCOL_INFORMATION: usize =
        InfoId::APPLICATION_PROTOCOL_INFORMATION as usize;
    pub const SERVER_NAME_INFORMATION: usize = InfoId::SERVER_NAME_INFORMATION as usize;
    pub const KEY_EXCHANGE_GROUP: usize = InfoId::KEY_EXCHANGE_GROUP as usize;
    pub const PACKET_SKIPPED: usize = InfoId::PACKET_SKIPPED as usize;
    pub const PACKET_SENT: usize = InfoId::PACKET_SENT as usize;
    pub const PACKET_SENT__KIND: usize = InfoId::PACKET_SENT__KIND as usize;
    pub const PACKET_SENT__BYTES__TOTAL: usize = InfoId::PACKET_SENT__BYTES__TOTAL as usize;
    pub const PACKET_SENT__BYTES: usize = InfoId::PACKET_SENT__BYTES as usize;
    pub const PACKET_SENT__TRANSMISSION_MODE: usize =
        InfoId::PACKET_SENT__TRANSMISSION_MODE as usize;
    pub const PACKET_RECEIVED: usize = InfoId::PACKET_RECEIVED as usize;
    pub const PACKET_RECEIVED__KIND: usize = InfoId::PACKET_RECEIVED__KIND as usize;
    pub const PACKET_RECEIVED__BYTES__TOTAL: usize = InfoId::PACKET_RECEIVED__BYTES__TOTAL as usize;
    pub const PACKET_RECEIVED__BYTES: usize = InfoId::PACKET_RECEIVED__BYTES as usize;
    pub const ACTIVE_PATH_UPDATED: usize = InfoId::ACTIVE_PATH_UPDATED as usize;
    pub const PATH_CREATED: usize = InfoId::PATH_CREATED as usize;
    pub const FRAME_SENT: usize = InfoId::FRAME_SENT as usize;
    pub const FRAME_SENT__PACKET: usize = InfoId::FRAME_SENT__PACKET as usize;
    pub const FRAME_SENT__FRAME: usize = InfoId::FRAME_SENT__FRAME as usize;
    pub const FRAME_RECEIVED: usize = InfoId::FRAME_RECEIVED as usize;
    pub const FRAME_RECEIVED__PACKET: usize = InfoId::FRAME_RECEIVED__PACKET as usize;
    pub const FRAME_RECEIVED__FRAME: usize = InfoId::FRAME_RECEIVED__FRAME as usize;
    pub const CONNECTION_CLOSE_FRAME_RECEIVED: usize =
        InfoId::CONNECTION_CLOSE_FRAME_RECEIVED as usize;
    pub const CONNECTION_CLOSE_FRAME_RECEIVED__PACKET: usize =
        InfoId::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET as usize;
    pub const PACKET_LOST: usize = InfoId::PACKET_LOST as usize;
    pub const PACKET_LOST__KIND: usize = InfoId::PACKET_LOST__KIND as usize;
    pub const PACKET_LOST__BYTES__TOTAL: usize = InfoId::PACKET_LOST__BYTES__TOTAL as usize;
    pub const PACKET_LOST__BYTES: usize = InfoId::PACKET_LOST__BYTES as usize;
    pub const PACKET_LOST__IS_MTU_PROBE: usize = InfoId::PACKET_LOST__IS_MTU_PROBE as usize;
    pub const RECOVERY_METRICS: usize = InfoId::RECOVERY_METRICS as usize;
    pub const RECOVERY_METRICS__MIN_RTT: usize = InfoId::RECOVERY_METRICS__MIN_RTT as usize;
    pub const RECOVERY_METRICS__SMOOTHED_RTT: usize =
        InfoId::RECOVERY_METRICS__SMOOTHED_RTT as usize;
    pub const RECOVERY_METRICS__LATEST_RTT: usize = InfoId::RECOVERY_METRICS__LATEST_RTT as usize;
    pub const RECOVERY_METRICS__RTT_VARIANCE: usize =
        InfoId::RECOVERY_METRICS__RTT_VARIANCE as usize;
    pub const RECOVERY_METRICS__MAX_ACK_DELAY: usize =
        InfoId::RECOVERY_METRICS__MAX_ACK_DELAY as usize;
    pub const RECOVERY_METRICS__PTO_COUNT: usize = InfoId::RECOVERY_METRICS__PTO_COUNT as usize;
    pub const RECOVERY_METRICS__CONGESTION_WINDOW: usize =
        InfoId::RECOVERY_METRICS__CONGESTION_WINDOW as usize;
    pub const RECOVERY_METRICS__BYTES_IN_FLIGHT: usize =
        InfoId::RECOVERY_METRICS__BYTES_IN_FLIGHT as usize;
    pub const RECOVERY_METRICS__CONGESTION_LIMITED: usize =
        InfoId::RECOVERY_METRICS__CONGESTION_LIMITED as usize;
    pub const CONGESTION: usize = InfoId::CONGESTION as usize;
    pub const CONGESTION__SOURCE: usize = InfoId::CONGESTION__SOURCE as usize;
    pub const RX_ACK_RANGE_DROPPED: usize = InfoId::RX_ACK_RANGE_DROPPED as usize;
    pub const ACK_RANGE_RECEIVED: usize = InfoId::ACK_RANGE_RECEIVED as usize;
    pub const ACK_RANGE_RECEIVED__PACKET: usize = InfoId::ACK_RANGE_RECEIVED__PACKET as usize;
    pub const ACK_RANGE_SENT: usize = InfoId::ACK_RANGE_SENT as usize;
    pub const ACK_RANGE_SENT__PACKET: usize = InfoId::ACK_RANGE_SENT__PACKET as usize;
    pub const PACKET_DROPPED: usize = InfoId::PACKET_DROPPED as usize;
    pub const PACKET_DROPPED__REASON: usize = InfoId::PACKET_DROPPED__REASON as usize;
    pub const KEY_UPDATE: usize = InfoId::KEY_UPDATE as usize;
    pub const KEY_UPDATE__KEY_TYPE: usize = InfoId::KEY_UPDATE__KEY_TYPE as usize;
    pub const KEY_UPDATE__CIPHER_SUITE: usize = InfoId::KEY_UPDATE__CIPHER_SUITE as usize;
    pub const KEY_SPACE_DISCARDED: usize = InfoId::KEY_SPACE_DISCARDED as usize;
    pub const KEY_SPACE_DISCARDED__INITIAL__LATENCY: usize =
        InfoId::KEY_SPACE_DISCARDED__INITIAL__LATENCY as usize;
    pub const KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY: usize =
        InfoId::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY as usize;
    pub const KEY_SPACE_DISCARDED__ONE_RTT__LATENCY: usize =
        InfoId::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY as usize;
    pub const KEY_SPACE_DISCARDED__SPACE: usize = InfoId::KEY_SPACE_DISCARDED__SPACE as usize;
    pub const CONNECTION_STARTED: usize = InfoId::CONNECTION_STARTED as usize;
    pub const DUPLICATE_PACKET: usize = InfoId::DUPLICATE_PACKET as usize;
    pub const DUPLICATE_PACKET__KIND: usize = InfoId::DUPLICATE_PACKET__KIND as usize;
    pub const DUPLICATE_PACKET__ERROR: usize = InfoId::DUPLICATE_PACKET__ERROR as usize;
    pub const TRANSPORT_PARAMETERS_RECEIVED: usize = InfoId::TRANSPORT_PARAMETERS_RECEIVED as usize;
    pub const TRANSPORT_PARAMETERS_RECEIVED__LATENCY: usize =
        InfoId::TRANSPORT_PARAMETERS_RECEIVED__LATENCY as usize;
    pub const DATAGRAM_SENT: usize = InfoId::DATAGRAM_SENT as usize;
    pub const DATAGRAM_SENT__BYTES__TOTAL: usize = InfoId::DATAGRAM_SENT__BYTES__TOTAL as usize;
    pub const DATAGRAM_SENT__BYTES: usize = InfoId::DATAGRAM_SENT__BYTES as usize;
    pub const DATAGRAM_SENT__GSO_OFFSET: usize = InfoId::DATAGRAM_SENT__GSO_OFFSET as usize;
    pub const DATAGRAM_RECEIVED: usize = InfoId::DATAGRAM_RECEIVED as usize;
    pub const DATAGRAM_RECEIVED__BYTES__TOTAL: usize =
        InfoId::DATAGRAM_RECEIVED__BYTES__TOTAL as usize;
    pub const DATAGRAM_RECEIVED__BYTES: usize = InfoId::DATAGRAM_RECEIVED__BYTES as usize;
    pub const DATAGRAM_DROPPED: usize = InfoId::DATAGRAM_DROPPED as usize;
    pub const DATAGRAM_DROPPED__BYTES__TOTAL: usize =
        InfoId::DATAGRAM_DROPPED__BYTES__TOTAL as usize;
    pub const DATAGRAM_DROPPED__BYTES: usize = InfoId::DATAGRAM_DROPPED__BYTES as usize;
    pub const DATAGRAM_DROPPED__REASON: usize = InfoId::DATAGRAM_DROPPED__REASON as usize;
    pub const HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED: usize =
        InfoId::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED as usize;
    pub const CONNECTION_ID_UPDATED: usize = InfoId::CONNECTION_ID_UPDATED as usize;
    pub const ECN_STATE_CHANGED: usize = InfoId::ECN_STATE_CHANGED as usize;
    pub const ECN_STATE_CHANGED__STATE: usize = InfoId::ECN_STATE_CHANGED__STATE as usize;
    pub const CONNECTION_MIGRATION_DENIED: usize = InfoId::CONNECTION_MIGRATION_DENIED as usize;
    pub const CONNECTION_MIGRATION_DENIED__REASON: usize =
        InfoId::CONNECTION_MIGRATION_DENIED__REASON as usize;
    pub const HANDSHAKE_STATUS_UPDATED: usize = InfoId::HANDSHAKE_STATUS_UPDATED as usize;
    pub const HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY: usize =
        InfoId::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY as usize;
    pub const HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY: usize =
        InfoId::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY as usize;
    pub const HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY: usize =
        InfoId::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY as usize;
    pub const HANDSHAKE_STATUS_UPDATED__STATUS: usize =
        InfoId::HANDSHAKE_STATUS_UPDATED__STATUS as usize;
    pub const TLS_EXPORTER_READY: usize = InfoId::TLS_EXPORTER_READY as usize;
    pub const TLS_HANDSHAKE_FAILED: usize = InfoId::TLS_HANDSHAKE_FAILED as usize;
    pub const PATH_CHALLENGE_UPDATED: usize = InfoId::PATH_CHALLENGE_UPDATED as usize;
    pub const PATH_CHALLENGE_UPDATED__STATUS: usize =
        InfoId::PATH_CHALLENGE_UPDATED__STATUS as usize;
    pub const TLS_CLIENT_HELLO: usize = InfoId::TLS_CLIENT_HELLO as usize;
    pub const TLS_CLIENT_HELLO__LATENCY: usize = InfoId::TLS_CLIENT_HELLO__LATENCY as usize;
    pub const TLS_SERVER_HELLO: usize = InfoId::TLS_SERVER_HELLO as usize;
    pub const TLS_SERVER_HELLO__LATENCY: usize = InfoId::TLS_SERVER_HELLO__LATENCY as usize;
    pub const RX_STREAM_PROGRESS: usize = InfoId::RX_STREAM_PROGRESS as usize;
    pub const RX_STREAM_PROGRESS__BYTES__TOTAL: usize =
        InfoId::RX_STREAM_PROGRESS__BYTES__TOTAL as usize;
    pub const RX_STREAM_PROGRESS__BYTES: usize = InfoId::RX_STREAM_PROGRESS__BYTES as usize;
    pub const TX_STREAM_PROGRESS: usize = InfoId::TX_STREAM_PROGRESS as usize;
    pub const TX_STREAM_PROGRESS__BYTES__TOTAL: usize =
        InfoId::TX_STREAM_PROGRESS__BYTES__TOTAL as usize;
    pub const TX_STREAM_PROGRESS__BYTES: usize = InfoId::TX_STREAM_PROGRESS__BYTES as usize;
    pub const KEEP_ALIVE_TIMER_EXPIRED: usize = InfoId::KEEP_ALIVE_TIMER_EXPIRED as usize;
    pub const MTU_UPDATED: usize = InfoId::MTU_UPDATED as usize;
    pub const MTU_UPDATED__MTU: usize = InfoId::MTU_UPDATED__MTU as usize;
    pub const MTU_UPDATED__CAUSE: usize = InfoId::MTU_UPDATED__CAUSE as usize;
    pub const MTU_UPDATED__SEARCH_COMPLETE: usize = InfoId::MTU_UPDATED__SEARCH_COMPLETE as usize;
    pub const MTU_PROBING_COMPLETE_RECEIVED: usize = InfoId::MTU_PROBING_COMPLETE_RECEIVED as usize;
    pub const MTU_PROBING_COMPLETE_RECEIVED__PACKET: usize =
        InfoId::MTU_PROBING_COMPLETE_RECEIVED__PACKET as usize;
    pub const MTU_PROBING_COMPLETE_RECEIVED__MTU: usize =
        InfoId::MTU_PROBING_COMPLETE_RECEIVED__MTU as usize;
    pub const SLOW_START_EXITED: usize = InfoId::SLOW_START_EXITED as usize;
    pub const SLOW_START_EXITED__CAUSE: usize = InfoId::SLOW_START_EXITED__CAUSE as usize;
    pub const SLOW_START_EXITED__LATENCY: usize = InfoId::SLOW_START_EXITED__LATENCY as usize;
    pub const SLOW_START_EXITED__CONGESTION_WINDOW: usize =
        InfoId::SLOW_START_EXITED__CONGESTION_WINDOW as usize;
    pub const DELIVERY_RATE_SAMPLED: usize = InfoId::DELIVERY_RATE_SAMPLED as usize;
    pub const PACING_RATE_UPDATED: usize = InfoId::PACING_RATE_UPDATED as usize;
    pub const PACING_RATE_UPDATED__BYTES_PER_SECOND: usize =
        InfoId::PACING_RATE_UPDATED__BYTES_PER_SECOND as usize;
    pub const PACING_RATE_UPDATED__BURST_SIZE: usize =
        InfoId::PACING_RATE_UPDATED__BURST_SIZE as usize;
    pub const PACING_RATE_UPDATED__PACING_GAIN: usize =
        InfoId::PACING_RATE_UPDATED__PACING_GAIN as usize;
    pub const BBR_STATE_CHANGED: usize = InfoId::BBR_STATE_CHANGED as usize;
    pub const BBR_STATE_CHANGED__STATE: usize = InfoId::BBR_STATE_CHANGED__STATE as usize;
    pub const DC_STATE_CHANGED: usize = InfoId::DC_STATE_CHANGED as usize;
    pub const DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY: usize =
        InfoId::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY as usize;
    pub const DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY: usize =
        InfoId::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY as usize;
    pub const DC_STATE_CHANGED__PATH_SECRETS__LATENCY: usize =
        InfoId::DC_STATE_CHANGED__PATH_SECRETS__LATENCY as usize;
    pub const DC_STATE_CHANGED__COMPLETE__LATENCY: usize =
        InfoId::DC_STATE_CHANGED__COMPLETE__LATENCY as usize;
    pub const DC_STATE_CHANGED__STATE: usize = InfoId::DC_STATE_CHANGED__STATE as usize;
    pub const DC_PATH_CREATED: usize = InfoId::DC_PATH_CREATED as usize;
    pub const CONNECTION_CLOSED: usize = InfoId::CONNECTION_CLOSED as usize;
    pub const CONNECTION_CLOSED__LATENCY: usize = InfoId::CONNECTION_CLOSED__LATENCY as usize;
    pub const CONNECTION_CLOSED__ERROR: usize = InfoId::CONNECTION_CLOSED__ERROR as usize;
    pub const VERSION_INFORMATION: usize = InfoId::VERSION_INFORMATION as usize;
    pub const ENDPOINT_PACKET_SENT: usize = InfoId::ENDPOINT_PACKET_SENT as usize;
    pub const ENDPOINT_PACKET_RECEIVED: usize = InfoId::ENDPOINT_PACKET_RECEIVED as usize;
    pub const ENDPOINT_DATAGRAM_SENT: usize = InfoId::ENDPOINT_DATAGRAM_SENT as usize;
    pub const ENDPOINT_DATAGRAM_SENT__BYTES: usize = InfoId::ENDPOINT_DATAGRAM_SENT__BYTES as usize;
    pub const ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL: usize =
        InfoId::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL as usize;
    pub const ENDPOINT_DATAGRAM_SENT__GSO_OFFSET: usize =
        InfoId::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET as usize;
    pub const ENDPOINT_DATAGRAM_RECEIVED: usize = InfoId::ENDPOINT_DATAGRAM_RECEIVED as usize;
    pub const ENDPOINT_DATAGRAM_RECEIVED__BYTES: usize =
        InfoId::ENDPOINT_DATAGRAM_RECEIVED__BYTES as usize;
    pub const ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL: usize =
        InfoId::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL as usize;
    pub const ENDPOINT_DATAGRAM_DROPPED: usize = InfoId::ENDPOINT_DATAGRAM_DROPPED as usize;
    pub const ENDPOINT_DATAGRAM_DROPPED__BYTES: usize =
        InfoId::ENDPOINT_DATAGRAM_DROPPED__BYTES as usize;
    pub const ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL: usize =
        InfoId::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL as usize;
    pub const ENDPOINT_DATAGRAM_DROPPED__REASON: usize =
        InfoId::ENDPOINT_DATAGRAM_DROPPED__REASON as usize;
    pub const ENDPOINT_CONNECTION_ATTEMPT_FAILED: usize =
        InfoId::ENDPOINT_CONNECTION_ATTEMPT_FAILED as usize;
    pub const ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR: usize =
        InfoId::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR as usize;
    pub const ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED: usize =
        InfoId::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED as usize;
    pub const PLATFORM_TX: usize = InfoId::PLATFORM_TX as usize;
    pub const PLATFORM_TX__PACKETS__TOTAL: usize = InfoId::PLATFORM_TX__PACKETS__TOTAL as usize;
    pub const PLATFORM_TX__PACKETS: usize = InfoId::PLATFORM_TX__PACKETS as usize;
    pub const PLATFORM_TX__SYSCALLS__TOTAL: usize = InfoId::PLATFORM_TX__SYSCALLS__TOTAL as usize;
    pub const PLATFORM_TX__SYSCALLS: usize = InfoId::PLATFORM_TX__SYSCALLS as usize;
    pub const PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL: usize =
        InfoId::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL as usize;
    pub const PLATFORM_TX__SYSCALLS__BLOCKED: usize =
        InfoId::PLATFORM_TX__SYSCALLS__BLOCKED as usize;
    pub const PLATFORM_TX__ERRORS__TOTAL: usize = InfoId::PLATFORM_TX__ERRORS__TOTAL as usize;
    pub const PLATFORM_TX__ERRORS: usize = InfoId::PLATFORM_TX__ERRORS as usize;
    pub const PLATFORM_TX__ERRORS__DROPPED__TOTAL: usize =
        InfoId::PLATFORM_TX__ERRORS__DROPPED__TOTAL as usize;
    pub const PLATFORM_TX__ERRORS__DROPPED: usize = InfoId::PLATFORM_TX__ERRORS__DROPPED as usize;
    pub const PLATFORM_TX_ERROR: usize = InfoId::PLATFORM_TX_ERROR as usize;
    pub const PLATFORM_RX: usize = InfoId::PLATFORM_RX as usize;
    pub const PLATFORM_RX__PACKETS__TOTAL: usize = InfoId::PLATFORM_RX__PACKETS__TOTAL as usize;
    pub const PLATFORM_RX__PACKETS: usize = InfoId::PLATFORM_RX__PACKETS as usize;
    pub const PLATFORM_RX__SYSCALLS__TOTAL: usize = InfoId::PLATFORM_RX__SYSCALLS__TOTAL as usize;
    pub const PLATFORM_RX__SYSCALLS: usize = InfoId::PLATFORM_RX__SYSCALLS as usize;
    pub const PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL: usize =
        InfoId::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL as usize;
    pub const PLATFORM_RX__SYSCALLS__BLOCKED: usize =
        InfoId::PLATFORM_RX__SYSCALLS__BLOCKED as usize;
    pub const PLATFORM_RX__ERRORS__TOTAL: usize = InfoId::PLATFORM_RX__ERRORS__TOTAL as usize;
    pub const PLATFORM_RX__ERRORS: usize = InfoId::PLATFORM_RX__ERRORS as usize;
    pub const PLATFORM_RX__ERRORS__DROPPED__TOTAL: usize =
        InfoId::PLATFORM_RX__ERRORS__DROPPED__TOTAL as usize;
    pub const PLATFORM_RX__ERRORS__DROPPED: usize = InfoId::PLATFORM_RX__ERRORS__DROPPED as usize;
    pub const PLATFORM_RX_ERROR: usize = InfoId::PLATFORM_RX_ERROR as usize;
    pub const PLATFORM_FEATURE_CONFIGURED: usize = InfoId::PLATFORM_FEATURE_CONFIGURED as usize;
    pub const PLATFORM_RX_SOCKET_STATS: usize = InfoId::PLATFORM_RX_SOCKET_STATS as usize;
    pub const PLATFORM_RX_SOCKET_STATS__PACKETS__TOTAL: usize =
        InfoId::PLATFORM_RX_SOCKET_STATS__PACKETS__TOTAL as usize;
    pub const PLATFORM_EVENT_LOOP_WAKEUP: usize = InfoId::PLATFORM_EVENT_LOOP_WAKEUP as usize;
    pub const PLATFORM_EVENT_LOOP_SLEEP: usize = InfoId::PLATFORM_EVENT_LOOP_SLEEP as usize;
    pub const PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION: usize =
        InfoId::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION as usize;
    pub const PLATFORM_EVENT_LOOP_STARTED: usize = InfoId::PLATFORM_EVENT_LOOP_STARTED as usize;
}
mod counter {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            match info.id {
                id::APPLICATION_PROTOCOL_INFORMATION => Self(application_protocol_information),
                id::SERVER_NAME_INFORMATION => Self(server_name_information),
                id::KEY_EXCHANGE_GROUP => Self(key_exchange_group),
                id::PACKET_SKIPPED => Self(packet_skipped),
                id::PACKET_SENT => Self(packet_sent),
                id::PACKET_SENT__BYTES__TOTAL => Self(packet_sent__bytes__total),
                id::PACKET_RECEIVED => Self(packet_received),
                id::PACKET_RECEIVED__BYTES__TOTAL => Self(packet_received__bytes__total),
                id::ACTIVE_PATH_UPDATED => Self(active_path_updated),
                id::PATH_CREATED => Self(path_created),
                id::FRAME_SENT => Self(frame_sent),
                id::FRAME_RECEIVED => Self(frame_received),
                id::CONNECTION_CLOSE_FRAME_RECEIVED => Self(connection_close_frame_received),
                id::PACKET_LOST => Self(packet_lost),
                id::PACKET_LOST__BYTES__TOTAL => Self(packet_lost__bytes__total),
                id::RECOVERY_METRICS => Self(recovery_metrics),
                id::CONGESTION => Self(congestion),
                id::RX_ACK_RANGE_DROPPED => Self(rx_ack_range_dropped),
                id::ACK_RANGE_RECEIVED => Self(ack_range_received),
                id::ACK_RANGE_SENT => Self(ack_range_sent),
                id::PACKET_DROPPED => Self(packet_dropped),
                id::KEY_UPDATE => Self(key_update),
                id::KEY_SPACE_DISCARDED => Self(key_space_discarded),
                id::CONNECTION_STARTED => Self(connection_started),
                id::DUPLICATE_PACKET => Self(duplicate_packet),
                id::TRANSPORT_PARAMETERS_RECEIVED => Self(transport_parameters_received),
                id::DATAGRAM_SENT => Self(datagram_sent),
                id::DATAGRAM_SENT__BYTES__TOTAL => Self(datagram_sent__bytes__total),
                id::DATAGRAM_RECEIVED => Self(datagram_received),
                id::DATAGRAM_RECEIVED__BYTES__TOTAL => Self(datagram_received__bytes__total),
                id::DATAGRAM_DROPPED => Self(datagram_dropped),
                id::DATAGRAM_DROPPED__BYTES__TOTAL => Self(datagram_dropped__bytes__total),
                id::HANDSHAKE_REMOTE_ADDRESS_CHANGE_OBSERVED => {
                    Self(handshake_remote_address_change_observed)
                }
                id::CONNECTION_ID_UPDATED => Self(connection_id_updated),
                id::ECN_STATE_CHANGED => Self(ecn_state_changed),
                id::CONNECTION_MIGRATION_DENIED => Self(connection_migration_denied),
                id::HANDSHAKE_STATUS_UPDATED => Self(handshake_status_updated),
                id::TLS_EXPORTER_READY => Self(tls_exporter_ready),
                id::TLS_HANDSHAKE_FAILED => Self(tls_handshake_failed),
                id::PATH_CHALLENGE_UPDATED => Self(path_challenge_updated),
                id::TLS_CLIENT_HELLO => Self(tls_client_hello),
                id::TLS_SERVER_HELLO => Self(tls_server_hello),
                id::RX_STREAM_PROGRESS => Self(rx_stream_progress),
                id::RX_STREAM_PROGRESS__BYTES__TOTAL => Self(rx_stream_progress__bytes__total),
                id::TX_STREAM_PROGRESS => Self(tx_stream_progress),
                id::TX_STREAM_PROGRESS__BYTES__TOTAL => Self(tx_stream_progress__bytes__total),
                id::KEEP_ALIVE_TIMER_EXPIRED => Self(keep_alive_timer_expired),
                id::MTU_UPDATED => Self(mtu_updated),
                id::MTU_PROBING_COMPLETE_RECEIVED => Self(mtu_probing_complete_received),
                id::SLOW_START_EXITED => Self(slow_start_exited),
                id::DELIVERY_RATE_SAMPLED => Self(delivery_rate_sampled),
                id::PACING_RATE_UPDATED => Self(pacing_rate_updated),
                id::BBR_STATE_CHANGED => Self(bbr_state_changed),
                id::DC_STATE_CHANGED => Self(dc_state_changed),
                id::DC_PATH_CREATED => Self(dc_path_created),
                id::CONNECTION_CLOSED => Self(connection_closed),
                id::VERSION_INFORMATION => Self(version_information),
                id::ENDPOINT_PACKET_SENT => Self(endpoint_packet_sent),
                id::ENDPOINT_PACKET_RECEIVED => Self(endpoint_packet_received),
                id::ENDPOINT_DATAGRAM_SENT => Self(endpoint_datagram_sent),
                id::ENDPOINT_DATAGRAM_RECEIVED => Self(endpoint_datagram_received),
                id::ENDPOINT_DATAGRAM_DROPPED => Self(endpoint_datagram_dropped),
                id::ENDPOINT_CONNECTION_ATTEMPT_FAILED => Self(endpoint_connection_attempt_failed),
                id::ENDPOINT_CONNECTION_ATTEMPT_DEDUPLICATED => {
                    Self(endpoint_connection_attempt_deduplicated)
                }
                id::PLATFORM_TX => Self(platform_tx),
                id::PLATFORM_TX__PACKETS__TOTAL => Self(platform_tx__packets__total),
                id::PLATFORM_TX__SYSCALLS__TOTAL => Self(platform_tx__syscalls__total),
                id::PLATFORM_TX__SYSCALLS__BLOCKED__TOTAL => {
                    Self(platform_tx__syscalls__blocked__total)
                }
                id::PLATFORM_TX__ERRORS__TOTAL => Self(platform_tx__errors__total),
                id::PLATFORM_TX__ERRORS__DROPPED__TOTAL => {
                    Self(platform_tx__errors__dropped__total)
                }
                id::PLATFORM_TX_ERROR => Self(platform_tx_error),
                id::PLATFORM_RX => Self(platform_rx),
                id::PLATFORM_RX__PACKETS__TOTAL => Self(platform_rx__packets__total),
                id::PLATFORM_RX__SYSCALLS__TOTAL => Self(platform_rx__syscalls__total),
                id::PLATFORM_RX__SYSCALLS__BLOCKED__TOTAL => {
                    Self(platform_rx__syscalls__blocked__total)
                }
                id::PLATFORM_RX__ERRORS__TOTAL => Self(platform_rx__errors__total),
                id::PLATFORM_RX__ERRORS__DROPPED__TOTAL => {
                    Self(platform_rx__errors__dropped__total)
                }
                id::PLATFORM_RX_ERROR => Self(platform_rx_error),
                id::PLATFORM_FEATURE_CONFIGURED => Self(platform_feature_configured),
                id::PLATFORM_RX_SOCKET_STATS => Self(platform_rx_socket_stats),
                id::PLATFORM_RX_SOCKET_STATS__PACKETS__TOTAL => {
                    Self(platform_rx_socket_stats__packets__total)
                }
                id::PLATFORM_EVENT_LOOP_WAKEUP => Self(platform_event_loop_wakeup),
                id::PLATFORM_EVENT_LOOP_SLEEP => Self(platform_event_loop_sleep),
                id::PLATFORM_EVENT_LOOP_STARTED => Self(platform_event_loop_started),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic__event__counter__application_protocol_information]
            fn application_protocol_information(value: u64);
            # [link_name = s2n_quic__event__counter__server_name_information]
            fn server_name_information(value: u64);
            # [link_name = s2n_quic__event__counter__key_exchange_group]
            fn key_exchange_group(value: u64);
            # [link_name = s2n_quic__event__counter__packet_skipped]
            fn packet_skipped(value: u64);
            # [link_name = s2n_quic__event__counter__packet_sent]
            fn packet_sent(value: u64);
            # [link_name = s2n_quic__event__counter__packet_sent__bytes__total]
            fn packet_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__packet_received]
            fn packet_received(value: u64);
            # [link_name = s2n_quic__event__counter__packet_received__bytes__total]
            fn packet_received__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__active_path_updated]
            fn active_path_updated(value: u64);
            # [link_name = s2n_quic__event__counter__path_created]
            fn path_created(value: u64);
            # [link_name = s2n_quic__event__counter__frame_sent]
            fn frame_sent(value: u64);
            # [link_name = s2n_quic__event__counter__frame_received]
            fn frame_received(value: u64);
            # [link_name = s2n_quic__event__counter__connection_close_frame_received]
            fn connection_close_frame_received(value: u64);
            # [link_name = s2n_quic__event__counter__packet_lost]
            fn packet_lost(value: u64);
            # [link_name = s2n_quic__event__counter__packet_lost__bytes__total]
            fn packet_lost__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__recovery_metrics]
            fn recovery_metrics(value: u64);
            # [link_name = s2n_quic__event__counter__congestion]
            fn congestion(value: u64);
            # [link_name = s2n_quic__event__counter__rx_ack_range_dropped]
            fn rx_ack_range_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__ack_range_received]
            fn ack_range_received(value: u64);
            # [link_name = s2n_quic__event__counter__ack_range_sent]
            fn ack_range_sent(value: u64);
            # [link_name = s2n_quic__event__counter__packet_dropped]
            fn packet_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__key_update]
            fn key_update(value: u64);
            # [link_name = s2n_quic__event__counter__key_space_discarded]
            fn key_space_discarded(value: u64);
            # [link_name = s2n_quic__event__counter__connection_started]
            fn connection_started(value: u64);
            # [link_name = s2n_quic__event__counter__duplicate_packet]
            fn duplicate_packet(value: u64);
            # [link_name = s2n_quic__event__counter__transport_parameters_received]
            fn transport_parameters_received(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_sent]
            fn datagram_sent(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_sent__bytes__total]
            fn datagram_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_received]
            fn datagram_received(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_received__bytes__total]
            fn datagram_received__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_dropped]
            fn datagram_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__datagram_dropped__bytes__total]
            fn datagram_dropped__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__handshake_remote_address_change_observed]
            fn handshake_remote_address_change_observed(value: u64);
            # [link_name = s2n_quic__event__counter__connection_id_updated]
            fn connection_id_updated(value: u64);
            # [link_name = s2n_quic__event__counter__ecn_state_changed]
            fn ecn_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__connection_migration_denied]
            fn connection_migration_denied(value: u64);
            # [link_name = s2n_quic__event__counter__handshake_status_updated]
            fn handshake_status_updated(value: u64);
            # [link_name = s2n_quic__event__counter__tls_exporter_ready]
            fn tls_exporter_ready(value: u64);
            # [link_name = s2n_quic__event__counter__tls_handshake_failed]
            fn tls_handshake_failed(value: u64);
            # [link_name = s2n_quic__event__counter__path_challenge_updated]
            fn path_challenge_updated(value: u64);
            # [link_name = s2n_quic__event__counter__tls_client_hello]
            fn tls_client_hello(value: u64);
            # [link_name = s2n_quic__event__counter__tls_server_hello]
            fn tls_server_hello(value: u64);
            # [link_name = s2n_quic__event__counter__rx_stream_progress]
            fn rx_stream_progress(value: u64);
            # [link_name = s2n_quic__event__counter__rx_stream_progress__bytes__total]
            fn rx_stream_progress__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__tx_stream_progress]
            fn tx_stream_progress(value: u64);
            # [link_name = s2n_quic__event__counter__tx_stream_progress__bytes__total]
            fn tx_stream_progress__bytes__total(value: u64);
            # [link_name = s2n_quic__event__counter__keep_alive_timer_expired]
            fn keep_alive_timer_expired(value: u64);
            # [link_name = s2n_quic__event__counter__mtu_updated]
            fn mtu_updated(value: u64);
            # [link_name = s2n_quic__event__counter__mtu_probing_complete_received]
            fn mtu_probing_complete_received(value: u64);
            # [link_name = s2n_quic__event__counter__slow_start_exited]
            fn slow_start_exited(value: u64);
            # [link_name = s2n_quic__event__counter__delivery_rate_sampled]
            fn delivery_rate_sampled(value: u64);
            # [link_name = s2n_quic__event__counter__pacing_rate_updated]
            fn pacing_rate_updated(value: u64);
            # [link_name = s2n_quic__event__counter__bbr_state_changed]
            fn bbr_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__dc_state_changed]
            fn dc_state_changed(value: u64);
            # [link_name = s2n_quic__event__counter__dc_path_created]
            fn dc_path_created(value: u64);
            # [link_name = s2n_quic__event__counter__connection_closed]
            fn connection_closed(value: u64);
            # [link_name = s2n_quic__event__counter__version_information]
            fn version_information(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_packet_sent]
            fn endpoint_packet_sent(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_packet_received]
            fn endpoint_packet_received(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_sent]
            fn endpoint_datagram_sent(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_received]
            fn endpoint_datagram_received(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_datagram_dropped]
            fn endpoint_datagram_dropped(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_connection_attempt_failed]
            fn endpoint_connection_attempt_failed(value: u64);
            # [link_name = s2n_quic__event__counter__endpoint_connection_attempt_deduplicated]
            fn endpoint_connection_attempt_deduplicated(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx]
            fn platform_tx(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__packets__total]
            fn platform_tx__packets__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__syscalls__total]
            fn platform_tx__syscalls__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__syscalls__blocked__total]
            fn platform_tx__syscalls__blocked__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__errors__total]
            fn platform_tx__errors__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx__errors__dropped__total]
            fn platform_tx__errors__dropped__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_tx_error]
            fn platform_tx_error(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx]
            fn platform_rx(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__packets__total]
            fn platform_rx__packets__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__syscalls__total]
            fn platform_rx__syscalls__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__syscalls__blocked__total]
            fn platform_rx__syscalls__blocked__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__errors__total]
            fn platform_rx__errors__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx__errors__dropped__total]
            fn platform_rx__errors__dropped__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx_error]
            fn platform_rx_error(value: u64);
            # [link_name = s2n_quic__event__counter__platform_feature_configured]
            fn platform_feature_configured(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx_socket_stats]
            fn platform_rx_socket_stats(value: u64);
            # [link_name = s2n_quic__event__counter__platform_rx_socket_stats__packets__total]
            fn platform_rx_socket_stats__packets__total(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_wakeup]
            fn platform_event_loop_wakeup(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_sleep]
            fn platform_event_loop_sleep(value: u64);
            # [link_name = s2n_quic__event__counter__platform_event_loop_started]
            fn platform_event_loop_started(value: u64);
        }
    );
    pub mod bool {
        #![allow(non_snake_case)]
        use super::*;
        pub struct Recorder(fn(bool));
        impl Recorder {
            pub(crate) fn new(info: &'static Info) -> Self {
                match info.id {
                    id::PACKET_LOST__IS_MTU_PROBE => Self(packet_lost__is_mtu_probe),
                    id::RECOVERY_METRICS__CONGESTION_LIMITED => {
                        Self(recovery_metrics__congestion_limited)
                    }
                    id::MTU_UPDATED__SEARCH_COMPLETE => Self(mtu_updated__search_complete),
                    _ => unreachable!("invalid info: {info:?}"),
                }
            }
        }
        impl BoolRecorder for Recorder {
            fn record(&self, _info: &'static Info, value: bool) {
                (self.0)(value);
            }
        }
        define!(
            extern "probe" {
                # [link_name = s2n_quic__event__counter__bool__packet_lost__is_mtu_probe]
                fn packet_lost__is_mtu_probe(value: bool);
                # [link_name = s2n_quic__event__counter__bool__recovery_metrics__congestion_limited]
                fn recovery_metrics__congestion_limited(value: bool);
                # [link_name = s2n_quic__event__counter__bool__mtu_updated__search_complete]
                fn mtu_updated__search_complete(value: bool);
            }
        );
    }
    pub mod nominal {
        #![allow(non_snake_case)]
        use super::*;
        use crate::event::metrics::aggregate::Metric;
        pub struct Recorder(fn(u64, u64, &info::Str));
        impl Recorder {
            pub(crate) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                match info.id {
                    id::PACKET_SENT__KIND => Self(packet_sent__kind),
                    id::PACKET_SENT__TRANSMISSION_MODE => Self(packet_sent__transmission_mode),
                    id::PACKET_RECEIVED__KIND => Self(packet_received__kind),
                    id::FRAME_SENT__PACKET => Self(frame_sent__packet),
                    id::FRAME_SENT__FRAME => Self(frame_sent__frame),
                    id::FRAME_RECEIVED__PACKET => Self(frame_received__packet),
                    id::FRAME_RECEIVED__FRAME => Self(frame_received__frame),
                    id::CONNECTION_CLOSE_FRAME_RECEIVED__PACKET => {
                        Self(connection_close_frame_received__packet)
                    }
                    id::PACKET_LOST__KIND => Self(packet_lost__kind),
                    id::CONGESTION__SOURCE => Self(congestion__source),
                    id::ACK_RANGE_RECEIVED__PACKET => Self(ack_range_received__packet),
                    id::ACK_RANGE_SENT__PACKET => Self(ack_range_sent__packet),
                    id::PACKET_DROPPED__REASON => Self(packet_dropped__reason),
                    id::KEY_UPDATE__KEY_TYPE => Self(key_update__key_type),
                    id::KEY_UPDATE__CIPHER_SUITE => Self(key_update__cipher_suite),
                    id::KEY_SPACE_DISCARDED__SPACE => Self(key_space_discarded__space),
                    id::DUPLICATE_PACKET__KIND => Self(duplicate_packet__kind),
                    id::DUPLICATE_PACKET__ERROR => Self(duplicate_packet__error),
                    id::DATAGRAM_DROPPED__REASON => Self(datagram_dropped__reason),
                    id::ECN_STATE_CHANGED__STATE => Self(ecn_state_changed__state),
                    id::CONNECTION_MIGRATION_DENIED__REASON => {
                        Self(connection_migration_denied__reason)
                    }
                    id::HANDSHAKE_STATUS_UPDATED__STATUS => Self(handshake_status_updated__status),
                    id::PATH_CHALLENGE_UPDATED__STATUS => Self(path_challenge_updated__status),
                    id::MTU_UPDATED__CAUSE => Self(mtu_updated__cause),
                    id::MTU_PROBING_COMPLETE_RECEIVED__PACKET => {
                        Self(mtu_probing_complete_received__packet)
                    }
                    id::SLOW_START_EXITED__CAUSE => Self(slow_start_exited__cause),
                    id::BBR_STATE_CHANGED__STATE => Self(bbr_state_changed__state),
                    id::DC_STATE_CHANGED__STATE => Self(dc_state_changed__state),
                    id::CONNECTION_CLOSED__ERROR => Self(connection_closed__error),
                    id::ENDPOINT_DATAGRAM_DROPPED__REASON => {
                        Self(endpoint_datagram_dropped__reason)
                    }
                    id::ENDPOINT_CONNECTION_ATTEMPT_FAILED__ERROR => {
                        Self(endpoint_connection_attempt_failed__error)
                    }
                    _ => unreachable!("invalid info: {info:?}"),
                }
            }
        }
        impl NominalRecorder for Recorder {
            fn record<T: Metric>(
                &self,
                _info: &'static Info,
                variant: &'static info::Variant,
                value: T,
            ) {
                (self.0)(value.as_u64(), variant.id as _, variant.name);
            }
        }
        define!(
            extern "probe" {
                # [link_name = s2n_quic__event__counter__nominal__packet_sent__kind]
                fn packet_sent__kind(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__packet_sent__transmission_mode]
                fn packet_sent__transmission_mode(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__packet_received__kind]
                fn packet_received__kind(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__frame_sent__packet]
                fn frame_sent__packet(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__frame_sent__frame]
                fn frame_sent__frame(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__frame_received__packet]
                fn frame_received__packet(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__frame_received__frame]
                fn frame_received__frame(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__connection_close_frame_received__packet]
                fn connection_close_frame_received__packet(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__packet_lost__kind]
                fn packet_lost__kind(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__congestion__source]
                fn congestion__source(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__ack_range_received__packet]
                fn ack_range_received__packet(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__ack_range_sent__packet]
                fn ack_range_sent__packet(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__packet_dropped__reason]
                fn packet_dropped__reason(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__key_update__key_type]
                fn key_update__key_type(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__key_update__cipher_suite]
                fn key_update__cipher_suite(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__key_space_discarded__space]
                fn key_space_discarded__space(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__duplicate_packet__kind]
                fn duplicate_packet__kind(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__duplicate_packet__error]
                fn duplicate_packet__error(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__datagram_dropped__reason]
                fn datagram_dropped__reason(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__ecn_state_changed__state]
                fn ecn_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__connection_migration_denied__reason]
                fn connection_migration_denied__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__handshake_status_updated__status]
                fn handshake_status_updated__status(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__path_challenge_updated__status]
                fn path_challenge_updated__status(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__mtu_updated__cause]
                fn mtu_updated__cause(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__mtu_probing_complete_received__packet]
                fn mtu_probing_complete_received__packet(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__slow_start_exited__cause]
                fn slow_start_exited__cause(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__bbr_state_changed__state]
                fn bbr_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__dc_state_changed__state]
                fn dc_state_changed__state(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__connection_closed__error]
                fn connection_closed__error(value: u64, variant: u64, variant_name: &info::Str);
                # [link_name = s2n_quic__event__counter__nominal__endpoint_datagram_dropped__reason]
                fn endpoint_datagram_dropped__reason(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
                # [link_name = s2n_quic__event__counter__nominal__endpoint_connection_attempt_failed__error]
                fn endpoint_connection_attempt_failed__error(
                    value: u64,
                    variant: u64,
                    variant_name: &info::Str,
                );
            }
        );
    }
}
mod measure {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            match info.id {
                id::PACKET_SENT__BYTES => Self(packet_sent__bytes),
                id::PACKET_RECEIVED__BYTES => Self(packet_received__bytes),
                id::PACKET_LOST__BYTES => Self(packet_lost__bytes),
                id::RECOVERY_METRICS__MIN_RTT => Self(recovery_metrics__min_rtt),
                id::RECOVERY_METRICS__SMOOTHED_RTT => Self(recovery_metrics__smoothed_rtt),
                id::RECOVERY_METRICS__LATEST_RTT => Self(recovery_metrics__latest_rtt),
                id::RECOVERY_METRICS__RTT_VARIANCE => Self(recovery_metrics__rtt_variance),
                id::RECOVERY_METRICS__MAX_ACK_DELAY => Self(recovery_metrics__max_ack_delay),
                id::RECOVERY_METRICS__PTO_COUNT => Self(recovery_metrics__pto_count),
                id::RECOVERY_METRICS__CONGESTION_WINDOW => {
                    Self(recovery_metrics__congestion_window)
                }
                id::RECOVERY_METRICS__BYTES_IN_FLIGHT => Self(recovery_metrics__bytes_in_flight),
                id::DATAGRAM_SENT__BYTES => Self(datagram_sent__bytes),
                id::DATAGRAM_SENT__GSO_OFFSET => Self(datagram_sent__gso_offset),
                id::DATAGRAM_RECEIVED__BYTES => Self(datagram_received__bytes),
                id::DATAGRAM_DROPPED__BYTES => Self(datagram_dropped__bytes),
                id::RX_STREAM_PROGRESS__BYTES => Self(rx_stream_progress__bytes),
                id::TX_STREAM_PROGRESS__BYTES => Self(tx_stream_progress__bytes),
                id::MTU_UPDATED__MTU => Self(mtu_updated__mtu),
                id::MTU_PROBING_COMPLETE_RECEIVED__MTU => Self(mtu_probing_complete_received__mtu),
                id::SLOW_START_EXITED__CONGESTION_WINDOW => {
                    Self(slow_start_exited__congestion_window)
                }
                id::PACING_RATE_UPDATED__BYTES_PER_SECOND => {
                    Self(pacing_rate_updated__bytes_per_second)
                }
                id::PACING_RATE_UPDATED__BURST_SIZE => Self(pacing_rate_updated__burst_size),
                id::PACING_RATE_UPDATED__PACING_GAIN => Self(pacing_rate_updated__pacing_gain),
                id::ENDPOINT_DATAGRAM_SENT__BYTES => Self(endpoint_datagram_sent__bytes),
                id::ENDPOINT_DATAGRAM_SENT__BYTES__TOTAL => {
                    Self(endpoint_datagram_sent__bytes__total)
                }
                id::ENDPOINT_DATAGRAM_SENT__GSO_OFFSET => Self(endpoint_datagram_sent__gso_offset),
                id::ENDPOINT_DATAGRAM_RECEIVED__BYTES => Self(endpoint_datagram_received__bytes),
                id::ENDPOINT_DATAGRAM_RECEIVED__BYTES__TOTAL => {
                    Self(endpoint_datagram_received__bytes__total)
                }
                id::ENDPOINT_DATAGRAM_DROPPED__BYTES => Self(endpoint_datagram_dropped__bytes),
                id::ENDPOINT_DATAGRAM_DROPPED__BYTES__TOTAL => {
                    Self(endpoint_datagram_dropped__bytes__total)
                }
                id::PLATFORM_TX__PACKETS => Self(platform_tx__packets),
                id::PLATFORM_TX__SYSCALLS => Self(platform_tx__syscalls),
                id::PLATFORM_TX__SYSCALLS__BLOCKED => Self(platform_tx__syscalls__blocked),
                id::PLATFORM_TX__ERRORS => Self(platform_tx__errors),
                id::PLATFORM_TX__ERRORS__DROPPED => Self(platform_tx__errors__dropped),
                id::PLATFORM_RX__PACKETS => Self(platform_rx__packets),
                id::PLATFORM_RX__SYSCALLS => Self(platform_rx__syscalls),
                id::PLATFORM_RX__SYSCALLS__BLOCKED => Self(platform_rx__syscalls__blocked),
                id::PLATFORM_RX__ERRORS => Self(platform_rx__errors),
                id::PLATFORM_RX__ERRORS__DROPPED => Self(platform_rx__errors__dropped),
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic__event__measure__packet_sent__bytes]
            fn packet_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__packet_received__bytes]
            fn packet_received__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__packet_lost__bytes]
            fn packet_lost__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__min_rtt]
            fn recovery_metrics__min_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__smoothed_rtt]
            fn recovery_metrics__smoothed_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__latest_rtt]
            fn recovery_metrics__latest_rtt(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__rtt_variance]
            fn recovery_metrics__rtt_variance(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__max_ack_delay]
            fn recovery_metrics__max_ack_delay(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__pto_count]
            fn recovery_metrics__pto_count(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__congestion_window]
            fn recovery_metrics__congestion_window(value: u64);
            # [link_name = s2n_quic__event__measure__recovery_metrics__bytes_in_flight]
            fn recovery_metrics__bytes_in_flight(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_sent__bytes]
            fn datagram_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_sent__gso_offset]
            fn datagram_sent__gso_offset(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_received__bytes]
            fn datagram_received__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__datagram_dropped__bytes]
            fn datagram_dropped__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__rx_stream_progress__bytes]
            fn rx_stream_progress__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__tx_stream_progress__bytes]
            fn tx_stream_progress__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__mtu_updated__mtu]
            fn mtu_updated__mtu(value: u64);
            # [link_name = s2n_quic__event__measure__mtu_probing_complete_received__mtu]
            fn mtu_probing_complete_received__mtu(value: u64);
            # [link_name = s2n_quic__event__measure__slow_start_exited__congestion_window]
            fn slow_start_exited__congestion_window(value: u64);
            # [link_name = s2n_quic__event__measure__pacing_rate_updated__bytes_per_second]
            fn pacing_rate_updated__bytes_per_second(value: u64);
            # [link_name = s2n_quic__event__measure__pacing_rate_updated__burst_size]
            fn pacing_rate_updated__burst_size(value: u64);
            # [link_name = s2n_quic__event__measure__pacing_rate_updated__pacing_gain]
            fn pacing_rate_updated__pacing_gain(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__bytes]
            fn endpoint_datagram_sent__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__bytes__total]
            fn endpoint_datagram_sent__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_sent__gso_offset]
            fn endpoint_datagram_sent__gso_offset(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_received__bytes]
            fn endpoint_datagram_received__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_received__bytes__total]
            fn endpoint_datagram_received__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_dropped__bytes]
            fn endpoint_datagram_dropped__bytes(value: u64);
            # [link_name = s2n_quic__event__measure__endpoint_datagram_dropped__bytes__total]
            fn endpoint_datagram_dropped__bytes__total(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__packets]
            fn platform_tx__packets(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__syscalls]
            fn platform_tx__syscalls(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__syscalls__blocked]
            fn platform_tx__syscalls__blocked(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__errors]
            fn platform_tx__errors(value: u64);
            # [link_name = s2n_quic__event__measure__platform_tx__errors__dropped]
            fn platform_tx__errors__dropped(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__packets]
            fn platform_rx__packets(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__syscalls]
            fn platform_rx__syscalls(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__syscalls__blocked]
            fn platform_rx__syscalls__blocked(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__errors]
            fn platform_rx__errors(value: u64);
            # [link_name = s2n_quic__event__measure__platform_rx__errors__dropped]
            fn platform_rx__errors__dropped(value: u64);
        }
    );
}
mod gauge {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(u64));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            unreachable!("invalid info: {info:?}")
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_u64());
        }
    }
}
mod timer {
    #![allow(non_snake_case)]
    use super::*;
    use crate::event::metrics::aggregate::Metric;
    pub struct Recorder(fn(core::time::Duration));
    impl Recorder {
        pub(crate) fn new(info: &'static Info) -> Self {
            match info.id {
                id::KEY_SPACE_DISCARDED__INITIAL__LATENCY => {
                    Self(key_space_discarded__initial__latency)
                }
                id::KEY_SPACE_DISCARDED__HANDSHAKE__LATENCY => {
                    Self(key_space_discarded__handshake__latency)
                }
                id::KEY_SPACE_DISCARDED__ONE_RTT__LATENCY => {
                    Self(key_space_discarded__one_rtt__latency)
                }
                id::TRANSPORT_PARAMETERS_RECEIVED__LATENCY => {
                    Self(transport_parameters_received__latency)
                }
                id::HANDSHAKE_STATUS_UPDATED__COMPLETE__LATENCY => {
                    Self(handshake_status_updated__complete__latency)
                }
                id::HANDSHAKE_STATUS_UPDATED__CONFIRMED__LATENCY => {
                    Self(handshake_status_updated__confirmed__latency)
                }
                id::HANDSHAKE_STATUS_UPDATED__HANDSHAKE_DONE_ACKED__LATENCY => {
                    Self(handshake_status_updated__handshake_done_acked__latency)
                }
                id::TLS_CLIENT_HELLO__LATENCY => Self(tls_client_hello__latency),
                id::TLS_SERVER_HELLO__LATENCY => Self(tls_server_hello__latency),
                id::DC_STATE_CHANGED__VERSION_NEGOTIATED__LATENCY => {
                    Self(dc_state_changed__version_negotiated__latency)
                }
                id::DC_STATE_CHANGED__NO_VERSION_NEGOTIATED__LATENCY => {
                    Self(dc_state_changed__no_version_negotiated__latency)
                }
                id::DC_STATE_CHANGED__PATH_SECRETS__LATENCY => {
                    Self(dc_state_changed__path_secrets__latency)
                }
                id::DC_STATE_CHANGED__COMPLETE__LATENCY => {
                    Self(dc_state_changed__complete__latency)
                }
                id::CONNECTION_CLOSED__LATENCY => Self(connection_closed__latency),
                id::PLATFORM_EVENT_LOOP_SLEEP__PROCESSING_DURATION => {
                    Self(platform_event_loop_sleep__processing_duration)
                }
                _ => unreachable!("invalid info: {info:?}"),
            }
        }
    }
    impl MetricRecorder for Recorder {
        fn record<T: Metric>(&self, _info: &'static Info, value: T) {
            (self.0)(value.as_duration());
        }
    }
    define!(
        extern "probe" {
            # [link_name = s2n_quic__event__timer__key_space_discarded__initial__latency]
            fn key_space_discarded__initial__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__key_space_discarded__handshake__latency]
            fn key_space_discarded__handshake__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__key_space_discarded__one_rtt__latency]
            fn key_space_discarded__one_rtt__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__transport_parameters_received__latency]
            fn transport_parameters_received__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__handshake_status_updated__complete__latency]
            fn handshake_status_updated__complete__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__handshake_status_updated__confirmed__latency]
            fn handshake_status_updated__confirmed__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__handshake_status_updated__handshake_done_acked__latency]
            fn handshake_status_updated__handshake_done_acked__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__tls_client_hello__latency]
            fn tls_client_hello__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__tls_server_hello__latency]
            fn tls_server_hello__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__dc_state_changed__version_negotiated__latency]
            fn dc_state_changed__version_negotiated__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__dc_state_changed__no_version_negotiated__latency]
            fn dc_state_changed__no_version_negotiated__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__dc_state_changed__path_secrets__latency]
            fn dc_state_changed__path_secrets__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__dc_state_changed__complete__latency]
            fn dc_state_changed__complete__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__connection_closed__latency]
            fn connection_closed__latency(value: core::time::Duration);
            # [link_name = s2n_quic__event__timer__platform_event_loop_sleep__processing_duration]
            fn platform_event_loop_sleep__processing_duration(value: core::time::Duration);
        }
    );
    pub mod nominal {
        #![allow(non_snake_case)]
        use super::*;
        use crate::event::metrics::aggregate::Metric;
        pub struct Recorder(fn(core::time::Duration, u64, &info::Str));
        impl Recorder {
            pub(crate) fn new(info: &'static Info, _variant: &'static info::Variant) -> Self {
                match info.id {
                    id::SLOW_START_EXITED__LATENCY => Self(slow_start_exited__latency),
                    _ => unreachable!("invalid info: {info:?}"),
                }
            }
        }
        impl NominalRecorder for Recorder {
            fn record<T: Metric>(
                &self,
                _info: &'static Info,
                variant: &'static info::Variant,
                value: T,
            ) {
                (self.0)(value.as_duration(), variant.id as _, variant.name);
            }
        }
        define!(
            extern "probe" {
                # [link_name = s2n_quic__event__timer__nominal__slow_start_exited__latency]
                fn slow_start_exited__latency(
                    value: core::time::Duration,
                    variant: u64,
                    variant_name: &info::Str,
                );
            }
        );
    }
}
#[derive(Default)]
pub struct Registry(());
impl aggregate::Registry for Registry {
    type Counter = counter::Recorder;
    type BoolCounter = counter::bool::Recorder;
    type NominalCounter = counter::nominal::Recorder;
    type Measure = measure::Recorder;
    type Gauge = gauge::Recorder;
    type Timer = timer::Recorder;
    type NominalTimer = timer::nominal::Recorder;
    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        counter::Recorder::new(info)
    }
    #[inline]
    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
        counter::bool::Recorder::new(info)
    }
    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        counter::nominal::Recorder::new(info, variant)
    }
    #[inline]
    fn register_measure(&self, info: &'static Info) -> Self::Measure {
        measure::Recorder::new(info)
    }
    #[inline]
    fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
        gauge::Recorder::new(info)
    }
    #[inline]
    fn register_timer(&self, info: &'static Info) -> Self::Timer {
        timer::Recorder::new(info)
    }
    #[inline]
    fn register_nominal_timer(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalTimer {
        timer::nominal::Recorder::new(info, variant)
    }
}
