// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stats::{Connection, Parameters};
use core::time::Duration;
use once_cell::sync::Lazy;
use s2n_quic::{
    connection,
    provider::{
        event,
        io::testing::{primary, time},
    },
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

static IDS: Lazy<Arc<AtomicU64>> = Lazy::new(Default::default);

static IS_OPEN: AtomicBool = AtomicBool::new(true);

pub fn is_open() -> bool {
    IS_OPEN.load(Ordering::Relaxed)
}

pub fn close() {
    IS_OPEN.store(false, Ordering::Relaxed);
}

#[derive(Clone, Debug)]
pub struct Events {
    params: Arc<DumpOnDrop<Parameters>>,
}

fn now() -> Duration {
    unsafe { time::now().as_duration() }
}

pub struct PrimaryContext<Inner> {
    #[allow(dead_code)]
    guard: primary::Guard,
    inner: Inner,
}

impl<Inner> core::ops::Deref for PrimaryContext<Inner> {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<Inner> core::ops::DerefMut for PrimaryContext<Inner> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

trait OrDefault<T> {
    fn or_default(&mut self) -> &mut T;
}

impl<T: Default> OrDefault<T> for Option<T> {
    fn or_default(&mut self) -> &mut T {
        if self.is_none() {
            *self = Some(T::default());
        }

        self.as_mut().unwrap()
    }
}

impl event::Subscriber for Events {
    type ConnectionContext = PrimaryContext<DumpOnDrop<Connection>>;

    fn create_connection_context(
        &mut self,
        meta: &event::ConnectionMeta,
        _info: &event::ConnectionInfo,
    ) -> Self::ConnectionContext {
        let seed = self.params.seed;
        let id = IDS.fetch_add(1, Ordering::Relaxed);

        let mut conn = Connection {
            seed,
            start_time: Some(now().into()),
            ..Default::default()
        };

        if matches!(
            meta.endpoint_type,
            event::events::EndpointType::Server { .. }
        ) {
            conn.server_id = Some(id);
        } else {
            conn.client_id = Some(id);
        }

        let inner = DumpOnDrop(conn);
        PrimaryContext {
            guard: primary::guard(),
            inner,
        }
    }

    #[inline]
    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::PacketSent,
    ) {
        context.tx.or_default().inc_packet(&event.packet_header);
    }

    #[inline]
    fn on_packet_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::PacketReceived,
    ) {
        context.rx.or_default().inc_packet(&event.packet_header);
    }

    #[inline]
    fn on_packet_lost(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::PacketLost,
    ) {
        context.loss.or_default().inc_packet(&event.packet_header);
    }

    #[inline]
    fn on_congestion(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        _event: &event::events::Congestion,
    ) {
        context.congestion += 1;
    }

    #[inline]
    fn on_recovery_metrics(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::RecoveryMetrics,
    ) {
        context.max_cwin = context.max_cwin.max(event.congestion_window as _);
        context.max_bytes_in_flight = context.max_bytes_in_flight.max(event.bytes_in_flight as _);
        context.max_rtt = Some(
            context
                .max_rtt
                .unwrap_or_default()
                .as_duration()
                .max(event.smoothed_rtt)
                .into(),
        );
        context.min_rtt = Some(
            context
                .min_rtt
                .map_or(event.min_rtt, |prev| prev.as_duration().min(event.min_rtt))
                .into(),
        );
        context.smoothed_rtt = Some(event.smoothed_rtt.into());
    }

    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::HandshakeStatusUpdated,
    ) {
        match event.status {
            event::events::HandshakeStatus::Complete { .. } => {
                context.handshake.or_default().complete = Some(now().into())
            }
            event::events::HandshakeStatus::Confirmed { .. } => {
                context.handshake.or_default().confirmed = Some(now().into())
            }
            _ => {}
        }
    }

    #[inline]
    fn on_frame_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::FrameSent,
    ) {
        context.tx.or_default().inc_frame(&event.frame);
    }

    #[inline]
    fn on_frame_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::FrameReceived,
    ) {
        context.rx.or_default().inc_frame(&event.frame);
    }

    #[inline]
    fn on_rx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &event::ConnectionMeta,
        event: &event::events::RxStreamProgress,
    ) {
        context
            .rx
            .or_default()
            .stream_progress(meta.timestamp, event.bytes);
    }

    #[inline]
    fn on_tx_stream_progress(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &event::ConnectionMeta,
        event: &event::events::TxStreamProgress,
    ) {
        context
            .tx
            .or_default()
            .stream_progress(meta.timestamp, event.bytes);
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &event::ConnectionMeta,
        event: &event::events::ConnectionClosed,
    ) {
        context.end_time = Some(meta.timestamp.duration_since_start().into());

        match event.error {
            connection::Error::Closed { .. } => {}
            connection::Error::Transport { code, .. } => {
                context.transport_error = Some(code.as_u64());
            }
            connection::Error::Application { error, .. } => {
                let error = *error;
                // if the application closed it with `0` then it's no error
                if error > 0 {
                    context.application_error = Some(error);
                }
            }
            connection::Error::IdleTimerExpired { .. } => context.idle_timer_error = true,
            connection::Error::MaxHandshakeDurationExceeded { .. } => {
                context.handshake_duration_exceeded_error = true
            }
            _ => context.unspecified_error = true,
        }
    }
}

impl Dump for Connection {
    fn dump(&mut self) {
        if self.seed == 0 {
            return;
        }

        if self.end_time.is_none() {
            self.end_time = Some(now().into());
        }

        dump(|io| self.write(io));
    }
}

impl From<Parameters> for Events {
    fn from(s: Parameters) -> Self {
        Self {
            params: Arc::new(DumpOnDrop(s)),
        }
    }
}

impl Dump for Parameters {
    fn dump(&mut self) {
        if self.seed == 0 {
            return;
        }

        if self.end_time.is_none() {
            self.end_time = Some(now().into());
        }

        dump(|io| self.write(io));
    }
}

pub trait Dump {
    fn dump(&mut self);
}

#[derive(Debug)]
pub struct DumpOnDrop<T: Dump>(T);

impl<T: Dump> core::ops::Deref for DumpOnDrop<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Dump> core::ops::DerefMut for DumpOnDrop<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Dump> Drop for DumpOnDrop<T> {
    fn drop(&mut self) {
        self.0.dump();
    }
}

pub fn dump<F: FnOnce(&mut std::io::StdoutLock) -> std::io::Result<()>>(f: F) {
    if !IS_OPEN.load(Ordering::Relaxed) {
        return;
    }

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    let res = f(&mut stdout);

    if res.is_err() {
        // close the process as the reader is no longer interested
        IS_OPEN.store(false, Ordering::Relaxed);
    }
}
