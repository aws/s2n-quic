// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator::Allocator,
    clock::Timer,
    event, msg,
    stream::{shared::ArcShared, socket::Socket, Actor},
};
use core::task::{Context, Poll};
use s2n_quic_core::{buffer, endpoint, ensure, ready, time::clock::Timer as _};
use std::{io, time::Duration};
use tracing::{debug, trace};

const INITIAL_TIMEOUT: Duration = Duration::from_millis(2);

mod waiting {
    use s2n_quic_core::state::{event, is};

    #[derive(Clone, Debug, Default, PartialEq)]
    pub enum State {
        PeekPacket,
        EpochTimeout,
        #[default]
        Cooldown,
        DataRecvd,
        Detached,
        Finished,
    }

    impl State {
        is!(is_peek_packet, PeekPacket);
        event! {
            on_peek_packet(PeekPacket => EpochTimeout);
            on_cooldown_elapsed(Cooldown => PeekPacket);
            on_epoch_unchanged(EpochTimeout => PeekPacket);
            on_application_progress(PeekPacket | EpochTimeout | Cooldown => Cooldown);
            on_application_detach(PeekPacket | EpochTimeout | Cooldown => Detached);
            on_data_received(PeekPacket | EpochTimeout | Cooldown => DataRecvd);
            on_finished(PeekPacket | EpochTimeout | Cooldown | Detached | DataRecvd => Finished);
        }
    }

    #[test]
    fn dot_test() {
        insta::assert_snapshot!(State::dot());
    }
}

#[repr(u8)]
pub(crate) enum ErrorCode {
    /// The application dropped the stream without errors
    None = 0,
    /// General error code for application-level errors
    Application = 1,
}

pub struct Worker<S, Sub>
where
    S: Socket,
    Sub: event::Subscriber,
{
    shared: ArcShared<Sub>,
    last_observed_epoch: u64,
    send_buffer: msg::send::Message,
    state: waiting::State,
    timer: Timer,
    backoff: u8,
    socket: S,
}

impl<S, Sub> Worker<S, Sub>
where
    S: Socket,
    Sub: event::Subscriber,
{
    #[inline]
    pub fn new(socket: S, shared: ArcShared<Sub>, endpoint: endpoint::Type) -> Self {
        let send_buffer = msg::send::Message::new(shared.read_remote_addr(), shared.gso.clone());
        let timer = Timer::new_with_timeout(&shared.clock, INITIAL_TIMEOUT);

        let state = match endpoint {
            // on the client we delay before reading from the socket
            endpoint::Type::Client => waiting::State::Cooldown,
            // on the server we need the application to read after accepting, otherwise the peer
            // won't know what our port is
            endpoint::Type::Server => waiting::State::EpochTimeout,
        };

        Self {
            shared,
            last_observed_epoch: 0,
            send_buffer,
            state,
            timer,
            backoff: 0,
            socket,
        }
    }

    #[inline]
    pub fn update_waker(&self, cx: &mut Context) {
        self.shared.receiver.worker_waker.update(cx.waker());
    }

    #[inline]
    pub fn poll(&mut self, cx: &mut Context) -> Poll<()> {
        if let Poll::Ready(Err(err)) = self.poll_flush_socket(cx) {
            tracing::error!(socket_error = ?err);
            // TODO should we return? if we get a send error it's most likely fatal
            return Poll::Ready(());
        }

        if let Poll::Ready(Err(err)) = self.poll_socket(cx) {
            tracing::error!(socket_error = ?err);
            // TODO should we return? if we get a recv error it's most likely fatal
            return Poll::Ready(());
        }

        // go until we get into the finished state
        if let waiting::State::Finished = &self.state {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn poll_socket(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        loop {
            match &self.state {
                waiting::State::PeekPacket => {
                    // check to see if the application is progressing before peeking the socket
                    ensure!(!self.is_application_progressing(), continue);

                    // check if we have something pending
                    ready!(self.shared.receiver.poll_peek_worker(
                        cx,
                        &self.socket,
                        &self.shared.clock,
                        &self.shared.subscriber,
                    ));

                    self.arm_timer();
                    self.state.on_peek_packet().unwrap();
                    continue;
                }
                waiting::State::EpochTimeout => {
                    // check to see if the application is progressing before checking the timer
                    ensure!(!self.is_application_progressing(), continue);

                    ready!(self.timer.poll_ready(cx));

                    // the application isn't making progress so emit the timer expired event
                    self.state.on_epoch_unchanged().unwrap();

                    // only log this message after the first observation
                    if self.last_observed_epoch > 0 {
                        debug!("application reading too slowly from socket");
                    }

                    // reset the backoff with the assumption that the application will go slow in
                    // the future
                    self.backoff = 0;

                    // drain the socket if the application isn't going fast enough
                    return self.poll_drain_recv_socket(cx);
                }
                waiting::State::Cooldown => {
                    // check to see if the application is progressing before checking the timer
                    ensure!(!self.is_application_progressing(), continue);

                    ready!(self.timer.poll_ready(cx));

                    // go back to waiting for a packet
                    self.state.on_cooldown_elapsed().unwrap();
                    continue;
                }
                waiting::State::Detached | waiting::State::DataRecvd => {
                    ready!(self.poll_drain_recv_socket(cx))?;
                }
                waiting::State::Finished => {
                    // nothing left to do
                    return Ok(()).into();
                }
            }
        }
    }

    #[inline]
    fn is_application_progressing(&mut self) -> bool {
        // check to see if the application shut down
        if let super::shared::ApplicationState::Closed { is_panicking } =
            self.shared.receiver.application_state()
        {
            if let Ok(Some(mut recv)) = self.shared.receiver.worker_try_lock() {
                // check to see if we have anything in the reassembler as well
                let is_buffer_empty = recv.payload_is_empty() && recv.reassembler.is_empty();

                let error = if !is_buffer_empty || is_panicking {
                    // we still had data in our buffer so notify the sender
                    ErrorCode::Application as u8
                } else {
                    // no error - the application is just going away
                    ErrorCode::None as u8
                };

                recv.receiver.stop_sending(error.into());

                // TODO arm the timer so we can clean up when we're done

                if recv.receiver.is_finished() {
                    let _ = self.state.on_finished();
                }
            }

            let _ = self.state.on_application_detach();

            return true;
        }

        let current_epoch = self.shared.receiver.application_epoch();

        // make sure the epoch has changed since we last saw it before cooling down
        ensure!(self.last_observed_epoch < current_epoch, false);

        // record the new observation
        self.last_observed_epoch = current_epoch;

        // the application is making progress since the packet is different - loop back to cooldown
        trace!("application is making progress");

        // delay when we read from the socket again to avoid spinning
        let _ = self.state.on_application_progress();
        self.arm_timer();

        // after successful progress from the application we want to intervene less
        self.backoff = (self.backoff + 1).min(10);

        true
    }

    #[inline]
    fn poll_drain_recv_socket(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        let mut should_transmit = false;
        let mut received_packets = 0;

        let _res = self.process_packets(cx, &mut received_packets, &mut should_transmit);

        ensure!(
            should_transmit,
            if received_packets == 0 {
                Poll::Pending
            } else {
                Ok(()).into()
            }
        );

        // send an ACK if needed
        if let Some(mut recv) = self.shared.receiver.worker_try_lock()? {
            // use the latest value rather than trying to transmit an old one
            if !self.send_buffer.is_empty() {
                let _ = self.send_buffer.drain();
            }

            recv.fill_transmit_queue(&self.shared, &mut self.send_buffer);

            if recv.receiver.state().is_data_received() {
                let _ = self.state.on_data_received();
            }

            if recv.receiver.is_finished() {
                let _ = self.state.on_finished();
            } else {
                // TODO update the timer so we get woken up on idle timeout
            }
        }

        ready!(self.poll_flush_socket(cx))?;

        Ok(()).into()
    }

    #[inline]
    fn process_packets(
        &mut self,
        cx: &mut Context,
        received_packets: &mut usize,
        should_transmit: &mut bool,
    ) -> io::Result<()> {
        // loop until we hit Pending from the socket
        loop {
            // try_lock the state before reading so we don't consume a packet the application is
            // about to read
            let Some(mut recv) = self.shared.receiver.worker_try_lock()? else {
                // if the application is locking the state then we don't want to transmit, since it
                // will do that for us
                *should_transmit = false;
                break;
            };

            // make sure to process any left over packets, if any
            if !recv.payload_is_empty() {
                *should_transmit |= recv.process_recv_buffer(
                    &mut buffer::writer::storage::Empty,
                    &self.shared,
                    self.socket.features(),
                );
            }

            let res = recv.poll_fill_recv_buffer(
                cx,
                Actor::Worker,
                &self.socket,
                &self.shared.clock,
                &self.shared.subscriber,
            );

            match res {
                Poll::Pending => break,
                Poll::Ready(res) => res?,
            };

            *received_packets += 1;

            // process the packet we just received
            *should_transmit |= recv.process_recv_buffer(
                &mut buffer::writer::storage::Empty,
                &self.shared,
                self.socket.features(),
            );
        }

        Ok(())
    }

    #[inline]
    fn poll_flush_socket(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        while !self.send_buffer.is_empty() {
            ready!(self.socket.poll_send_buffer(cx, &mut self.send_buffer))?;
        }

        Ok(()).into()
    }

    #[inline]
    fn arm_timer(&mut self) {
        // TODO do we derive this from RTT?
        let mut timeout = INITIAL_TIMEOUT;
        // don't back off on packet peeks
        if !self.state.is_peek_packet() {
            timeout *= (self.backoff as u32) + 1;
        }
        let now = self.shared.clock.get_time();
        let target = now + timeout;

        self.timer.update(target);
    }
}
