// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{handle::Transmission, Protocol, Socket, TransportFeatures};
use crate::{
    clock::Clock,
    event::{self, builder, EndpointPublisher, IntoEvent},
    msg::{addr::Addr, cmsg},
};
use core::task::{Context, Poll};
use s2n_quic_core::{inet::ExplicitCongestionNotification, ready, time::Timestamp};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
    ops::Deref,
};

#[derive(Clone)]
pub struct Events<S: Socket, Sub: event::Subscriber, Clk: Clock> {
    pub socket: S,
    pub subscriber: Sub,
    pub clock: Clk,
}

impl<S: Socket, Sub: event::Subscriber, Clk: Clock> Events<S, Sub, Clk> {
    pub fn new(socket: S, subscriber: Sub, clock: Clk) -> Self {
        Self {
            socket,
            subscriber,
            clock,
        }
    }

    fn publisher(
        &self,
        timestamp: Option<Timestamp>,
    ) -> event::EndpointPublisherSubscriber<'_, Sub> {
        let timestamp = timestamp.unwrap_or_else(|| self.clock.get_time());
        let meta = builder::EndpointMeta {
            timestamp: timestamp.into_event(),
        };
        event::EndpointPublisherSubscriber::new(meta, None, &self.subscriber)
    }

    #[inline(always)]
    fn publish_send_event(&self, addr: &Addr, buffer: &[IoSlice], result: &io::Result<usize>) {
        let publisher = self.publisher(None);

        match &result {
            Ok(buffer_size) => {
                let buffer_size = *buffer_size as u16;
                let segment_size = buffer.first().map_or(0, |s| s.len()) as u16;
                let segment_count = buffer.len() as u16;

                publisher.on_endpoint_udp_packet_transmitted(
                    builder::EndpointUdpPacketTransmitted {
                        peer_address: addr.get().into_event(),
                        buffer_size,
                        segment_size,
                        segment_count,
                    },
                );
            }
            Err(error) => {
                let buffer_size = buffer.iter().map(|s| s.len()).sum::<usize>() as u16;
                let segment_size = buffer.first().map_or(0, |s| s.len()) as u16;
                let segment_count = buffer.len() as u16;

                publisher.on_endpoint_udp_transmit_errored(builder::EndpointUdpTransmitErrored {
                    peer_address: addr.get().into_event(),
                    buffer_size,
                    segment_size,
                    segment_count,
                    error,
                });
            }
        }
    }
}

impl<S: Socket, Sub: event::Subscriber, Clk: Clock> Deref for Events<S, Sub, Clk> {
    type Target = S;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.socket
    }
}

impl<S: Socket, Sub: event::Subscriber, Clk: Clock> Socket for Events<S, Sub, Clk> {
    #[inline(always)]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    #[inline]
    fn protocol(&self) -> Protocol {
        self.socket.protocol()
    }

    #[inline(always)]
    fn features(&self) -> TransportFeatures {
        self.socket.features()
    }

    #[inline(always)]
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
        self.socket.poll_peek_len(cx)
    }

    #[inline(always)]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        let result = ready!(self.socket.poll_recv(cx, addr, cmsg, buffer));

        let publisher = self.publisher(None);

        match &result {
            Ok(buffer_size) => {
                let buffer_size = *buffer_size as u16;
                let segment_size = cmsg.segment_len();
                let segment_size = if segment_size == 0 {
                    buffer_size
                } else {
                    segment_size
                };
                let segment_count = buffer_size.div_ceil(segment_size);

                publisher.on_endpoint_udp_packet_received(builder::EndpointUdpPacketReceived {
                    peer_address: addr.get().into_event(),
                    buffer_size,
                    segment_size,
                    segment_count,
                });
            }
            Err(error) => {
                publisher
                    .on_endpoint_udp_receive_errored(builder::EndpointUdpReceiveErrored { error });
            }
        }

        Poll::Ready(result)
    }

    #[inline(always)]
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        let result = self.socket.try_send(addr, ecn, buffer);

        self.publish_send_event(addr, buffer, &result);

        result
    }

    #[inline]
    fn send_transmission(&self, msg: Transmission) {
        let info = msg
            .descriptors
            .front()
            .map(|(desc, _)| (desc.remote_address().get(), desc.len()));
        let buffer_size = msg.total_len;
        let segment_count = msg.descriptors.len() as u16;

        self.socket.send_transmission(msg);

        let publisher = self.publisher(None);

        if let Some((addr, segment_size)) = info {
            publisher.on_endpoint_udp_immediate_transmission_scheduled(
                builder::EndpointUdpImmediateTransmissionScheduled {
                    peer_address: addr.into_event(),
                    buffer_size,
                    segment_size,
                    segment_count,
                },
            );
        }
    }

    #[inline]
    fn send_transmission_batch(&self, batch: crate::stream::send::state::transmission::EntryQueue) {
        // TODO publish batch scheduling events
        self.socket.send_transmission_batch(batch);
    }

    #[inline(always)]
    fn poll_send(
        &self,
        cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        let result = ready!(self.socket.poll_send(cx, addr, ecn, buffer));

        self.publish_send_event(addr, buffer, &result);

        Poll::Ready(result)
    }

    #[inline(always)]
    fn send_finish(&self) -> io::Result<()> {
        self.socket.send_finish()
    }
}
