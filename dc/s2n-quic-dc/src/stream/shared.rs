// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::Clock,
    credentials::Credentials,
    event::{self, IntoEvent as _},
    stream::{
        recv::shared as recv,
        send::{application, shared as send},
    },
};
use core::{
    cell::UnsafeCell,
    ops,
    sync::atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering},
    time::Duration,
};
use s2n_quic_core::{
    ensure,
    inet::{IpAddress, SocketAddress},
    time::Timestamp,
};
use s2n_quic_platform::features;
use std::sync::Arc;

pub use crate::stream::crypto::Crypto;

#[derive(Clone, Copy, Debug)]
pub enum Half {
    Read,
    Write,
}

pub type ArcShared<Sub> = Arc<Shared<Sub, dyn Clock>>;

pub struct Shared<Subscriber, Clk>
where
    Subscriber: event::Subscriber,
    Clk: ?Sized + Clock,
{
    pub receiver: recv::State,
    pub sender: send::State,
    pub crypto: Crypto,
    pub common: Common<Subscriber, Clk>,
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
        half: Half,
        did_complete_handshake: bool,
    ) {
        if did_complete_handshake {
            /*
            // TODO only update this if this if we are done "handshaking"
            let remote_port = match half {
                Half::Read => &self.read_remote_port,
                Half::Write => &self.write_remote_port,
            };
            remote_port.store(remote_addr.port(), Ordering::Relaxed);
            */
            let _ = half;
            let remote_port = remote_addr.port();
            if remote_port != 0 {
                self.read_remote_port.store(remote_port, Ordering::Relaxed);
                self.write_remote_port.store(remote_port, Ordering::Relaxed);
            }
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
}

impl<Sub, C> Shared<Sub, C>
where
    Sub: event::Subscriber,
    C: ?Sized + Clock,
{
    #[inline]
    pub fn last_peer_activity(&self) -> Timestamp {
        let timestamp = self.last_peer_activity.load(Ordering::Relaxed);
        let timestamp = Duration::from_micros(timestamp);
        unsafe { Timestamp::from_duration(timestamp) }
    }

    #[inline]
    pub fn write_remote_addr(&self) -> SocketAddress {
        self.remote_ip()
            .with_port(self.common.write_remote_port.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn read_remote_addr(&self) -> SocketAddress {
        self.remote_ip()
            .with_port(self.common.read_remote_port.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn remote_ip(&self) -> IpAddress {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            *self.common.fixed.remote_ip.get()
        }
    }

    #[inline]
    pub fn application(&self) -> application::state::State {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            *self.common.fixed.application.get()
        }
    }

    #[inline]
    pub fn source_control_port(&self) -> u16 {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            *self.common.fixed.source_control_port.get()
        }
    }

    #[inline]
    pub fn credentials(&self) -> &Credentials {
        unsafe {
            // SAFETY: the fixed information doesn't change for the lifetime of the stream
            &*self.common.fixed.credentials.get()
        }
    }
}

impl<Sub, C> ops::Deref for Shared<Sub, C>
where
    Sub: event::Subscriber,
    C: ?Sized + Clock,
{
    type Target = Common<Sub, C>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

pub struct Common<Sub, Clk>
where
    Sub: event::Subscriber,
    Clk: ?Sized + Clock,
{
    pub gso: features::Gso,
    pub read_remote_port: AtomicU16,
    pub write_remote_port: AtomicU16,
    pub fixed: FixedValues,
    /// The last time we received a packet from the peer
    pub last_peer_activity: AtomicU64,
    pub closed_halves: AtomicU8,
    pub subscriber: Subscriber<Sub>,
    pub clock: Clk,
}

impl<Sub, Clk> Common<Sub, Clk>
where
    Sub: event::Subscriber,
    Clk: ?Sized + Clock,
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
    pub fn publisher(&self) -> event::ConnectionPublisherSubscriber<Sub> {
        self.publisher_with_timestamp(self.clock.get_time())
    }

    #[inline]
    pub fn publisher_with_timestamp(
        &self,
        timestamp: Timestamp,
    ) -> event::ConnectionPublisherSubscriber<Sub> {
        self.subscriber.publisher(timestamp)
    }

    #[inline]
    pub fn endpoint_publisher(
        &self,
        timestamp: Timestamp,
    ) -> event::EndpointPublisherSubscriber<Sub> {
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
    pub fn publisher(&self, timestamp: Timestamp) -> event::ConnectionPublisherSubscriber<Sub> {
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
    ) -> event::EndpointPublisherSubscriber<Sub> {
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
    Clk: ?Sized + Clock,
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
    pub source_control_port: UnsafeCell<u16>,
    pub application: UnsafeCell<application::state::State>,
    pub credentials: UnsafeCell<Credentials>,
}

unsafe impl Send for FixedValues {}
unsafe impl Sync for FixedValues {}
