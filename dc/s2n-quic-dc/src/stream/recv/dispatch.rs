// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials, packet, path,
    socket::recv::descriptor,
    stream::recv::buffer::Channel,
    sync::{mpmc, mpsc},
};
use s2n_quic_core::{inet::SocketAddress, varint::VarInt};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};

struct Free {
    items: Mutex<Vec<FreeRoute>>,
}

#[derive(Clone)]
pub struct Allocator {
    routes: Routes,
    free: Arc<Free>,
    epoch: u64,
}

impl Allocator {
    pub fn alloc(&mut self) -> Alloced {
        let route = loop {
            let mut free = self.free.items.lock().unwrap();

            if let Some(route) = free.pop() {
                break route;
            }

            // drop the lock while we allocate the channels
            drop(free);

            let mut slab = self.routes.write().unwrap();

            // if the epochs don't match then loop back around, since someone else just updated it
            if slab.epoch != self.epoch {
                continue;
            }

            let id = slab.pages.len() * ROUTE_PAGE_SIZE;
            let mut id = VarInt::new(id as _).unwrap();

            let mut pending_flush = vec![];

            let page = core::array::from_fn(|idx| {
                let (control, control_r) = mpsc::new(64);
                let (stream, stream_r) = mpsc::new(4096);

                let id = id + idx;

                pending_flush.push(FreeRoute {
                    id,
                    control: control_r,
                    stream: stream_r,
                });

                Route {
                    control,
                    stream,
                    is_active: AtomicBool::new(false),
                }
            });
            let page = RoutePage(Arc::new(page));
            slab.pages.push(page);

            // update the epoch
            self.epoch += 1;
            slab.epoch = self.epoch;

            // move any pending flushes the free list
            let mut free = self.free.items.lock().unwrap();
            drop(slab);

            free.append(&mut pending_flush);
            break free.pop().unwrap();
        };

        todo!()
    }
}

pub struct Alloced {
    pub route_key: VarInt,
    pub control: Channel,
    pub stream: Channel,
}

struct FreeRoute {
    id: VarInt,
    control: Arc<mpsc::Receiver<descriptor::Filled>>,
    stream: Arc<mpsc::Receiver<descriptor::Filled>>,
}

const ROUTE_PAGE_SIZE: usize = 256;

struct Route {
    control: mpsc::Sender<descriptor::Filled>,
    stream: mpsc::Sender<descriptor::Filled>,
    is_active: AtomicBool,
}

impl Route {
    #[inline]
    fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
struct RoutePage(Arc<[Route; ROUTE_PAGE_SIZE]>);

#[derive(Clone)]
struct RouteSlab {
    pages: Vec<RoutePage>,
    epoch: u64,
}

type Routes = Arc<RwLock<RouteSlab>>;

#[derive(Clone)]
pub struct Dispatch {
    routes: Routes,
    cached: Vec<RoutePage>,
    map: path::secret::Map,
}

impl Dispatch {
    #[inline]
    fn route(&mut self, id: VarInt) -> Option<&Route> {
        let id: usize = id.try_into().ok()?;

        let page_idx = id / ROUTE_PAGE_SIZE;
        let route_idx = id % ROUTE_PAGE_SIZE;

        // if the cached length is smaller than the target, try to synchronize with the global view
        if self.cached.len() <= page_idx {
            let routes = self.routes.read().ok()?;

            // the local cache is up to date
            if routes.pages.len() == self.cached.len() {
                return None;
            }

            // copy the missing routes from the global value to the local cache
            self.cached
                .extend(routes.pages[self.cached.len()..].iter().cloned());
        }

        let routes = self.cached.get(page_idx)?;
        let route = &routes.0[route_idx];

        if !route.is_active() {
            return None;
        }

        Some(route)
    }
}

impl crate::socket::recv::router::Router for Dispatch {
    #[inline(always)]
    fn is_open(&self) -> bool {
        // TODO
        true
    }

    #[inline(always)]
    fn tag_len(&self) -> usize {
        16
    }

    /// implement this so we don't get warnings about not handling it
    #[inline(always)]
    fn handle_control_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::control::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        _tag: packet::control::Tag,
        id: Option<packet::stream::Id>,
        _credentials: credentials::Credentials,
        segment: descriptor::Filled,
    ) {
        let Some(id) = id else {
            return;
        };
        let Some(route) = self.route(id.route_key) else {
            // TODO log that the message is unroutable
            return;
        };
        let _res = route.control.send_back(segment);
        // TODO log result if overflow/error
    }

    /// implement this so we don't get warnings about not handling it
    #[inline(always)]
    fn handle_stream_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::stream::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: packet::stream::Tag,
        id: packet::stream::Id,
        _credentials: credentials::Credentials,
        segment: descriptor::Filled,
    ) {
        let Some(route) = self.route(id.route_key) else {
            // TODO log that the message is unroutable
            return;
        };
        let _res = route.stream.send_back(segment);
        // TODO log result if overflow/error
    }

    #[inline]
    fn handle_stale_key_packet(
        &mut self,
        packet: packet::secret_control::stale_key::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline]
    fn handle_replay_detected_packet(
        &mut self,
        packet: packet::secret_control::replay_detected::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline]
    fn handle_unknown_path_secret_packet(
        &mut self,
        packet: packet::secret_control::unknown_path_secret::Packet,
        remote_address: SocketAddress,
    ) {
        self.map
            .handle_control_packet(&packet.into(), &remote_address.into());
    }

    #[inline(always)]
    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: SocketAddress,
        segment: descriptor::Filled,
    ) {
        tracing::warn!(
            ?error,
            ?remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}
