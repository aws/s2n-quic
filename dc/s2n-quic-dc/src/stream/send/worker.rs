// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::{Clock, Timer},
    msg,
    msg::addr,
    packet::Packet,
    stream::{
        pacer,
        send::{
            error::{self, Error},
            queue::Queue,
            shared::Event,
            state::State,
        },
        shared::{self, Half},
        socket::Socket,
    },
};
use core::task::{Context, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    endpoint, ensure,
    inet::ExplicitCongestionNotification,
    random, ready,
    recovery::bandwidth::Bandwidth,
    time::{
        clock::{self, Timer as _},
        timer::Provider as _,
        Timestamp,
    },
    varint::VarInt,
};
use std::sync::Arc;

mod waiting {
    use s2n_quic_core::state::event;

    #[derive(Clone, Debug, Default, PartialEq)]
    pub enum State {
        #[default]
        Acking,
        Detached,
        ShuttingDown,
        Finished,
    }

    impl State {
        event! {
            on_application_detach(Acking => Detached);
            on_shutdown(Acking | Detached => ShuttingDown);
            on_finished(ShuttingDown => Finished);
        }
    }

    #[test]
    fn dot_test() {
        insta::assert_snapshot!(State::dot());
    }
}

pub struct Worker<S: Socket, R: random::Generator, C: Clock> {
    shared: Arc<shared::Shared<C>>,
    sender: State,
    recv_buffer: msg::recv::Message,
    random: R,
    state: waiting::State,
    timer: Timer,
    application_queue: Queue,
    pacer: pacer::Naive,
    socket: S,
}

#[derive(Debug)]
struct Snapshot {
    flow_offset: VarInt,
    has_pending_retransmissions: bool,
    send_quantum: usize,
    max_datagram_size: u16,
    ecn: ExplicitCongestionNotification,
    next_expected_control_packet: VarInt,
    timeout: Option<Timestamp>,
    bandwidth: Bandwidth,
    error: Option<Error>,
}

impl Snapshot {
    #[inline]
    fn apply<C: Clock>(&self, initial: &Self, shared: &shared::Shared<C>) {
        if initial.flow_offset < self.flow_offset {
            shared.sender.flow.release(self.flow_offset);
        } else if initial.has_pending_retransmissions && !self.has_pending_retransmissions {
            // we were waiting to clear out our retransmission queue before giving the application
            // more flow credits
            shared.sender.flow.release_max(self.flow_offset);
        }

        if initial.send_quantum != self.send_quantum {
            let send_quantum = (self.send_quantum as u64 + self.max_datagram_size as u64 - 1)
                / self.max_datagram_size as u64;
            let send_quantum = send_quantum.try_into().unwrap_or(u8::MAX);
            shared
                .sender
                .path
                .update_info(self.ecn, send_quantum, self.max_datagram_size);
        }

        if initial.next_expected_control_packet < self.next_expected_control_packet {
            shared
                .sender
                .path
                .set_next_expected_control_packet(self.next_expected_control_packet);
        }

        if initial.bandwidth != self.bandwidth {
            shared.sender.set_bandwidth(self.bandwidth);
        }

        if let Some(error) = self.error {
            if initial.error.is_none() {
                shared.sender.flow.set_error(error);
            }
        }
    }
}

impl<S, R, C> Worker<S, R, C>
where
    S: Socket,
    R: random::Generator,
    C: Clock,
{
    #[inline]
    pub fn new(
        socket: S,
        random: R,
        shared: Arc<shared::Shared<C>>,
        mut sender: State,
        endpoint: endpoint::Type,
    ) -> Self {
        let timer = Timer::new(&shared.clock);
        let recv_buffer = msg::recv::Message::new(u16::MAX);
        let state = Default::default();

        // if this is a client then set up the sender
        if endpoint.is_client() {
            sender.init_client(&shared.clock);
        }

        Self {
            shared,
            sender,
            recv_buffer,
            random,
            state,
            timer,
            application_queue: Default::default(),
            pacer: Default::default(),
            socket,
        }
    }

    #[inline]
    pub fn update_waker(&self, cx: &mut Context) {
        self.shared.sender.worker_waker.update(cx.waker());
    }

    #[inline]
    pub fn poll(&mut self, cx: &mut Context) -> Poll<()> {
        let initial = self.snapshot();

        let is_initial = self.sender.state.is_ready();

        tracing::trace!(
            view = "before",
            sender_state = ?self.sender.state,
            worker_state = ?self.state,
            snapshot = ?initial,
        );

        self.shared.sender.worker_waker.on_worker_wake();

        self.poll_once(cx);

        // check if the application sent us any more messages
        if !self
            .shared
            .sender
            .worker_waker
            .on_worker_sleep()
            .is_working()
        {
            // yield to the runtime
            cx.waker().wake_by_ref();
        }

        let current = self.snapshot();

        tracing::trace!(
            view = "after",
            sender_state = ?self.sender.state,
            worker_state = ?self.state,
            snapshot = ?current,
        );

        if is_initial || initial.timeout != current.timeout {
            if let Some(target) = current.timeout {
                self.timer.update(target);
                if self.timer.poll_ready(cx).is_ready() {
                    cx.waker().wake_by_ref();
                }
            } else {
                self.timer.cancel();
            }
        }

        current.apply(&initial, &self.shared);

        if let waiting::State::Finished = &self.state {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn poll_once(&mut self, cx: &mut Context) {
        let _ = self.poll_messages(cx);
        let _ = self.poll_socket(cx);

        let _ = self.poll_timers(cx);
        let _ = self.poll_transmit(cx);
        self.after_transmit();
    }

    #[inline]
    fn poll_messages(&mut self, cx: &mut Context) -> Poll<()> {
        let _ = cx;

        while let Some(message) = self.shared.sender.pop_worker_message() {
            match message.event {
                Event::Shutdown {
                    queue,
                    is_panicking,
                } => {
                    // if the application is panicking then we notify the peer
                    if is_panicking {
                        let error = error::Kind::ApplicationError { error: 1u8.into() };
                        self.sender.on_error(error);
                        continue;
                    }

                    // transition to a detached state
                    if self.state.on_application_detach().is_ok() {
                        debug_assert!(
                            self.application_queue.is_empty(),
                            "dropped queue twice for same stream"
                        );

                        self.application_queue = queue;
                        continue;
                    }
                }
            }
        }

        Poll::Ready(())
    }

    #[inline]
    fn poll_socket(&mut self, cx: &mut Context) -> Poll<()> {
        loop {
            // try to receive until we get blocked
            let _ = ready!(self.socket.poll_recv_buffer(cx, &mut self.recv_buffer));
            self.process_recv_buffer();
        }
    }

    #[inline]
    fn process_recv_buffer(&mut self) {
        ensure!(!self.recv_buffer.is_empty());

        let remote_addr = self.recv_buffer.remote_address();
        let tag_len = self.shared.crypto.tag_len();
        let ecn = self.recv_buffer.ecn();
        let random = &mut self.random;
        let mut any_valid_packets = false;
        let clock = &clock::Cached::new(&self.shared.clock);
        let opener = self
            .shared
            .crypto
            .control_opener()
            .expect("control crypto should be available");

        for segment in self.recv_buffer.segments() {
            let segment_len = segment.len();
            let mut decoder = DecoderBufferMut::new(segment);

            while !decoder.is_empty() {
                let remaining_len = decoder.len();

                let packet = match decoder.decode_parameterized(tag_len) {
                    Ok((packet, remaining)) => {
                        decoder = remaining;
                        packet
                    }
                    Err(err) => {
                        // we couldn't parse the rest of the packet so bail
                        tracing::error!(decoder_error = %err, segment_len, remaining_len);
                        break;
                    }
                };

                match packet {
                    Packet::Control(mut packet) => {
                        // make sure we're processing the expected stream
                        ensure!(
                            packet.stream_id() == Some(&self.shared.application().stream_id),
                            continue
                        );

                        let res = self.sender.on_control_packet(
                            opener,
                            self.shared.credentials(),
                            ecn,
                            &mut packet,
                            random,
                            clock,
                            &self.shared.sender.application_transmission_queue,
                            &self.shared.sender.segment_alloc,
                        );

                        if res.is_ok() {
                            any_valid_packets = true;
                        }
                    }
                    other => self
                        .shared
                        .crypto
                        .map()
                        .handle_unexpected_packet(&other, &self.shared.write_remote_addr().into()),
                }
            }
        }

        if any_valid_packets {
            // if the writer saw any ACKs then we're done handshaking
            let did_complete_handshake = true;
            self.shared
                .on_valid_packet(&remote_addr, Half::Write, did_complete_handshake);
        }
    }

    #[inline]
    fn poll_timers(&mut self, cx: &mut Context) -> Poll<()> {
        let _ = cx;
        let shared = &self.shared;
        let clock = clock::Cached::new(&shared.clock);
        self.sender
            .on_time_update(&clock, || shared.last_peer_activity());
        Poll::Ready(())
    }

    #[inline]
    fn poll_transmit(&mut self, cx: &mut Context) -> Poll<()> {
        loop {
            ready!(self.poll_transmit_flush(cx));

            let control_sealer = self
                .shared
                .crypto
                .control_sealer()
                .expect("control crypto should be available");

            match self.state {
                waiting::State::Acking => {
                    let _ = self.sender.fill_transmit_queue(
                        control_sealer,
                        self.shared.credentials(),
                        self.socket.local_addr().unwrap().port(),
                        &self.shared.clock,
                    );
                }
                waiting::State::Detached => {
                    // flush the remaining application queue
                    let _ = ready!(self.application_queue.poll_flush(
                        cx,
                        usize::MAX,
                        &self.socket,
                        &addr::Addr::new(self.shared.write_remote_addr()),
                        &self.shared.sender.segment_alloc,
                        &self.shared.gso,
                    ));

                    // make sure we have the current view from the application
                    self.sender.load_transmission_queue(
                        &self.shared.sender.application_transmission_queue,
                    );

                    // try to transition to having sent all of the data
                    if self.sender.state.on_send_fin().is_ok() {
                        // arm the PTO now to force it to transmit a final packet
                        self.sender.pto.force_transmit();
                    }

                    // transition to shutting down
                    let _ = self.state.on_shutdown();

                    continue;
                }
                waiting::State::ShuttingDown => {
                    let _ = self.sender.fill_transmit_queue(
                        control_sealer,
                        self.shared.credentials(),
                        self.socket.local_addr().unwrap().port(),
                        &self.shared.clock,
                    );

                    if self.sender.state.is_terminal() {
                        let _ = self.state.on_finished();
                    }
                }
                waiting::State::Finished => break,
            }

            ensure!(!self.sender.transmit_queue.is_empty(), break);
        }

        Poll::Ready(())
    }

    #[inline]
    fn poll_transmit_flush(&mut self, cx: &mut Context) -> Poll<()> {
        ensure!(!self.sender.transmit_queue.is_empty(), Poll::Ready(()));

        let mut max_segments = self.shared.gso.max_segments();
        let addr = self.shared.write_remote_addr();
        let addr = addr::Addr::new(addr);
        let clock = &self.shared.clock;

        while !self.sender.transmit_queue.is_empty() {
            // pace out retransmissions
            ready!(self.pacer.poll_pacing(cx, &self.shared.clock));

            // construct all of the segments we're going to send in this batch
            let segments =
                msg::segment::Batch::new(self.sender.transmit_queue_iter(clock).take(max_segments));

            let ecn = segments.ecn();
            let res = ready!(self.socket.poll_send(cx, &addr, ecn, &segments));

            if let Err(error) = res {
                if self.shared.gso.handle_socket_error(&error).is_some() {
                    // update the max_segments value if it was changed due to the error
                    max_segments = 1;
                }
            }

            // consume the segments that we transmitted
            let segment_count = segments.len();
            drop(segments);
            self.sender.on_transmit_queue(segment_count);
        }

        Poll::Ready(())
    }

    #[inline]
    fn after_transmit(&mut self) {
        self.sender
            .load_transmission_queue(&self.shared.sender.application_transmission_queue);

        self.sender
            .before_sleep(&clock::Cached::new(&self.shared.clock));
    }

    #[inline]
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            flow_offset: self.sender.flow_offset(),
            has_pending_retransmissions: self.sender.transmit_queue.is_empty(),
            send_quantum: self.sender.cca.send_quantum(),
            // TODO get this from the ECN controller
            ecn: ExplicitCongestionNotification::Ect0,
            max_datagram_size: self.sender.max_datagram_size,
            next_expected_control_packet: self.sender.next_expected_control_packet,
            timeout: self.sender.next_expiration(),
            bandwidth: self.sender.cca.bandwidth(),
            error: self.sender.error,
        }
    }
}
