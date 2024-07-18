// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::Clock,
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

pub type ArcShared = Arc<Shared<dyn Clock>>;

#[derive(Debug)]
pub struct Shared<Clock: ?Sized> {
    pub receiver: recv::State,
    pub sender: send::State,
    pub crypto: Crypto,
    pub common: Common<Clock>,
}

impl<C: Clock + ?Sized> Shared<C> {
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

impl<C: ?Sized> Shared<C> {
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
}

impl<C: ?Sized> ops::Deref for Shared<C> {
    type Target = Common<C>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

#[derive(Debug)]
pub struct Common<Clock: ?Sized> {
    pub gso: features::Gso,
    pub read_remote_port: AtomicU16,
    pub write_remote_port: AtomicU16,
    pub fixed: FixedValues,
    /// The last time we received a packet from the peer
    pub last_peer_activity: AtomicU64,
    pub closed_halves: AtomicU8,
    pub clock: Clock,
}

impl<Clock: ?Sized> Common<Clock> {
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
}

/// Values that don't change while the state is shared between threads
#[derive(Debug)]
pub struct FixedValues {
    pub remote_ip: UnsafeCell<IpAddress>,
    pub source_control_port: UnsafeCell<u16>,
    pub application: UnsafeCell<application::state::State>,
}

unsafe impl Send for FixedValues {}
unsafe impl Sync for FixedValues {}
