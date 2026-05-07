// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::Clock,
    credentials::Credentials,
    event::{self, IntoEvent as _},
    packet::stream,
    socket::pool,
    stream::{
        error::{self, Error as StreamError, StoredError},
        recv::shared as recv,
        send::{application, shared as send},
        tls::S2nTlsConnection,
        Actor,
    },
    task::waker::worker::Waker as WorkerWaker,
};
use atomic_waker::AtomicWaker;
use core::{
    cell::UnsafeCell,
    ops,
    sync::atomic::{AtomicU64, AtomicU8, Ordering},
    time::Duration,
};
use s2n_quic_core::{
    endpoint::Location,
    ensure,
    inet::{IpAddress, SocketAddress},
    time::Timestamp,
    varint::VarInt,
};
use s2n_quic_platform::features;
use std::sync::{atomic::AtomicU16, Arc, OnceLock};

pub mod handshake;

pub use crate::stream::crypto::Crypto;

#[derive(Clone, Copy, Debug)]
pub enum Half {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy)]
pub enum ShutdownKind {
    Normal,
    Errored,
}

impl ShutdownKind {
    pub const ERRORED_CODE: u8 = 0x01;

    pub fn error_code(&self) -> Option<u8> {
        match self {
            ShutdownKind::Normal => None,
            ShutdownKind::Errored => Some(Self::ERRORED_CODE),
        }
    }
}

/// The state of whether the stream has been accepted by the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptState {
    Waiting,
    Accepted,
}

pub type ArcShared<Sub> = Arc<Shared<Sub, dyn Clock>>;

pub struct CompletionQueue<T>(UnsafeCell<Option<T>>);

impl<T: Clone> CompletionQueue<T> {
    pub fn uninit() -> Self {
        Self(UnsafeCell::new(None))
    }

    #[inline]
    pub unsafe fn set(&self, value: T) {
        let weak = unsafe { &mut *self.0.get() };
        *weak = Some(value);
    }

    pub unsafe fn load(&self) -> T {
        unsafe {
            let v = &*self.0.get();
            s2n_quic_core::assume!(v.is_some());
            v.clone().unwrap()
        }
    }
}

unsafe impl<T: Send> Send for CompletionQueue<T> {}
unsafe impl<T: Sync> Sync for CompletionQueue<T> {}

pub struct Shared<Subscriber, Clk>
where
    Subscriber: event::Subscriber,
    Clk: Clock + ?Sized,
{
    pub receiver: recv::State,
    pub sender: send::State,
    pub crypto: Crypto,
    pub common: Common<Subscriber, Clk>,
}

impl<S, Clk> crate::stream::send::state::transmission::Notify for Shared<S, Clk>
where
    S: event::Subscriber,
    Clk: Clock + ?Sized,
{
    fn complete(&self, entry: crate::stream::send::state::transmission::Entry) {
        match entry.meta.half {
            Half::Write => {
                self.sender.transmission_queue.complete_transmission(entry);
                self.common.wakers.write_worker_waker.wake();
            }
            Half::Read => {
                self.receiver
                    .transmission_queue
                    .complete_transmission(entry);
                self.common.wakers.read_worker_waker.wake();
            }
        }
    }
}

impl<Sub, C> Shared<Sub, C>
where
    Sub: event::Subscriber,
    C: Clock + ?Sized,
{
    #[inline]
    pub fn on_valid_packet(
        &self,
        remote_addr: &SocketAddress,
        remote_queue_id: Option<VarInt>,
        handshake: &mut handshake::State,
    ) {
        match handshake {
            handshake::State::ClientQueueIdObserved => {
                // allow the server to pick a different port on the first response
                self.remote_port
                    .store(remote_addr.port(), Ordering::Relaxed);

                // transition to steady state once the server provided its chosen `queue_id`
                if let Some(server_queue_id) = remote_queue_id {
                    self.remote_queue_id
                        .store(server_queue_id.as_u64(), Ordering::Relaxed);

                    let _ = handshake.on_observation_finished();
                }
            }
            handshake::State::ServerQueueIdObserved => {
                // no need to update the remote_queue_id value since we saw it on the first packet
                let _ = handshake.on_observation_finished();
            }
            _ => {}
        }

        // update the last time we've seen peer activity
        self.on_peer_activity();
    }

    #[inline]
    pub fn on_peer_activity(&self) {
        self.last_peer_activity.fetch_max(
            unsafe { self.clock.get_time().as_duration().as_micros() as _ },
            Ordering::Relaxed,
        );
    }

    #[inline]
    pub fn last_peer_activity(&self) -> Timestamp {
        let timestamp = self.last_peer_activity.load(Ordering::Relaxed);
        let timestamp = Duration::from_micros(timestamp);
        unsafe { Timestamp::from_duration(timestamp) }
    }

    #[inline]
    pub fn stream_id(&self) -> stream::Id {
        let queue_id = self.remote_queue_id();
        // TODO support alternative modes
        stream::Id {
            queue_id,
            is_reliable: true,
            is_bidirectional: true,
        }
    }

    #[inline]
    pub fn local_queue_id(&self) -> Option<VarInt> {
        let queue_id = self.local_queue_id.load(Ordering::Relaxed);
        VarInt::new(queue_id).ok()
    }

    #[inline]
    pub fn remote_queue_id(&self) -> VarInt {
        let queue_id = self.remote_queue_id.load(Ordering::Relaxed);
        unsafe { VarInt::new_unchecked(queue_id) }
    }

    #[inline]
    pub fn remote_addr(&self) -> SocketAddress {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            *self.common.fixed.remote_ip.get()
        }
        .with_port(self.remote_port.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn application(&self) -> application::state::State {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            *self.common.fixed.application.get()
        }
    }

    #[inline]
    pub fn credentials(&self) -> &Credentials {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            &*self.common.fixed.credentials.get()
        }
    }

    /// Stores a fatal error in the shared `OnceLock` and notifies all other actors.
    #[inline]
    pub fn set_error(&self, error: StreamError, source: Location, actor: Option<(Half, Actor)>) {
        // NormalClose (error_code=0) is a half-close signal, not a fatal error.
        // It means one direction is closing gracefully and should not poison the
        // entire bidirectional stream. Don't set the shared error for NormalClose.
        if matches!(error.kind(), error::Kind::NormalClose) {
            return;
        }

        let stored = StoredError { error, source };

        // First writer wins. If the error was already set, return immediately.
        if self.common.stream_error.set(stored).is_err() {
            return;
        }

        tracing::debug!(%error, ?source, ?actor, "setting stream error");

        // Indicate that an error has been encountered
        self.sender.set_error_flag();
        self.receiver.set_error_flag();

        // Wake all actors except the caller
        if let Some((half, actor)) = actor {
            self.common.wakers.wake_all_except(half, actor);
        } else {
            self.common.wakers.wake_all();
        }
    }

    /// Returns the shared error if one has been set.
    #[inline]
    pub fn get_error(&self) -> Option<&StoredError> {
        self.common.stream_error.get()
    }
}

impl<Sub, C> ops::Deref for Shared<Sub, C>
where
    Sub: event::Subscriber,
    C: Clock + ?Sized,
{
    type Target = Common<Sub, C>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

/// Consolidated waker set for all four actors interacting with the stream.
#[derive(Debug, Default)]
pub struct WakerSet {
    pub write_app_waker: AtomicWaker,
    pub write_worker_waker: WorkerWaker,
    pub read_app_waker: AtomicWaker,
    pub read_worker_waker: WorkerWaker,
}

impl WakerSet {
    pub fn wake_all(&self) {
        self.write_app_waker.wake();
        self.write_worker_waker.wake();
        self.read_app_waker.wake();
        self.read_worker_waker.wake();
    }

    /// Wakes all actors except the one identified by `half` and `actor`.
    /// This prevents the originator of an error from waking itself.
    pub fn wake_all_except(&self, half: Half, actor: Actor) {
        if !matches!((half, actor), (Half::Write, Actor::Application)) {
            self.write_app_waker.wake();
        }
        if !matches!((half, actor), (Half::Write, Actor::Worker)) {
            self.write_worker_waker.wake();
        }
        if !matches!((half, actor), (Half::Read, Actor::Application)) {
            self.read_app_waker.wake();
        }
        if !matches!((half, actor), (Half::Read, Actor::Worker)) {
            self.read_worker_waker.wake();
        }
    }
}

pub struct Common<Sub, Clk>
where
    Sub: event::Subscriber,
    Clk: Clock + ?Sized,
{
    pub gso: features::Gso,
    pub(super) remote_port: AtomicU16,
    pub(super) local_queue_id: AtomicU64,
    pub(super) remote_queue_id: AtomicU64,
    pub fixed: FixedValues,
    /// The last time we received a packet from the peer
    pub last_peer_activity: AtomicU64,
    pub closed_halves: AtomicU8,
    pub segment_alloc: pool::Pool,
    pub subscriber: Subscriber<Sub>,
    pub s2n_connection: Option<S2nTlsConnection>,
    pub stream_error: OnceLock<StoredError>,
    pub wakers: WakerSet,
    pub clock: Clk,
}

impl<Sub, Clk> Common<Sub, Clk>
where
    Sub: event::Subscriber,
    Clk: Clock + ?Sized,
{
    #[inline]
    pub fn ensure_open(&self) -> std::io::Result<()> {
        ensure!(
            self.closed_halves.load(Ordering::Relaxed) < 2,
            // macos returns a different error kind
            Err(if cfg!(target_os = "macos") {
                std::io::ErrorKind::InvalidInput
            } else {
                std::io::ErrorKind::NotConnected
            }
            .into())
        );
        Ok(())
    }

    #[inline]
    pub fn publisher(&self) -> event::ConnectionPublisherSubscriber<'_, Sub> {
        self.publisher_with_timestamp(self.clock.get_time())
    }

    #[inline]
    pub fn publisher_with_timestamp(
        &self,
        timestamp: Timestamp,
    ) -> event::ConnectionPublisherSubscriber<'_, Sub> {
        self.subscriber.publisher(timestamp)
    }

    #[inline]
    pub fn endpoint_publisher(
        &self,
        timestamp: Timestamp,
    ) -> event::EndpointPublisherSubscriber<'_, Sub> {
        self.subscriber.endpoint_publisher(timestamp)
    }
}

pub struct Subscriber<Sub>
where
    Sub: event::Subscriber,
{
    pub subscriber: Sub,
    pub context: Sub::ConnectionContext,
}

impl<Sub> Subscriber<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn publisher(&self, timestamp: Timestamp) -> event::ConnectionPublisherSubscriber<'_, Sub> {
        event::ConnectionPublisherSubscriber::new(
            event::builder::ConnectionMeta {
                id: 0, // TODO
                timestamp: timestamp.into_event(),
            },
            0,
            &self.subscriber,
            &self.context,
        )
    }

    #[inline]
    pub fn endpoint_publisher(
        &self,
        timestamp: Timestamp,
    ) -> event::EndpointPublisherSubscriber<'_, Sub> {
        event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                timestamp: timestamp.into_event(),
            },
            None,
            &self.subscriber,
        )
    }
}

impl<Sub, Clk> Drop for Common<Sub, Clk>
where
    Sub: event::Subscriber,
    Clk: Clock + ?Sized,
{
    #[inline]
    fn drop(&mut self) {
        use event::ConnectionPublisher as _;

        self.publisher()
            .on_connection_closed(event::builder::ConnectionClosed {});
    }
}

/// Values that don't change while the state is shared between threads
pub struct FixedValues {
    pub remote_ip: UnsafeCell<IpAddress>,
    pub application: UnsafeCell<application::state::State>,
    pub credentials: UnsafeCell<Credentials>,
}

unsafe impl Send for FixedValues {}
unsafe impl Sync for FixedValues {}
