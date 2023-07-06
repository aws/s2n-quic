// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher},
    io::{rx::Rx, tx::Tx},
    task::cooldown::Cooldown,
    time::clock::{ClockWithTimer, Timer},
};
use core::pin::Pin;

pub mod select;
use select::Select;

pub struct EventLoop<E, C, R, T> {
    pub endpoint: E,
    pub clock: C,
    pub rx: R,
    pub tx: T,
    pub cooldown: Cooldown,
}

impl<E, C, R, T> EventLoop<E, C, R, T>
where
    E: Endpoint,
    C: ClockWithTimer,
    R: Rx<PathHandle = E::PathHandle>,
    T: Tx<PathHandle = E::PathHandle>,
{
    /// Starts running the endpoint event loop in an async task
    pub async fn start(self) {
        let Self {
            mut endpoint,
            clock,
            mut rx,
            mut tx,
            mut cooldown,
        } = self;

        /// Creates a event publisher with the endpoint's subscriber
        macro_rules! publisher {
            ($timestamp:expr) => {{
                let timestamp = $timestamp;
                let subscriber = endpoint.subscriber();
                event::EndpointPublisherSubscriber::new(
                    event::builder::EndpointMeta {
                        endpoint_type: E::ENDPOINT_TYPE,
                        timestamp,
                    },
                    None,
                    subscriber,
                )
            }};
        }

        let mut timer = clock.timer();

        loop {
            // Poll for RX readiness
            let rx_ready = rx.ready();

            // Poll for TX readiness
            let tx_ready = tx.ready();

            // Poll for any application-driven updates
            let mut wakeups = endpoint.wakeups(&clock);

            // TODO use the [pin macro](https://doc.rust-lang.org/std/pin/macro.pin.html) once
            // available in MSRV
            //
            // See https://github.com/aws/s2n-quic/issues/1751
            let wakeups = unsafe {
                // Safety: the wakeups future is on the stack and won't move
                Pin::new_unchecked(&mut wakeups)
            };

            // Poll for timer expiration
            let timer_ready = timer.ready();

            // Concurrently poll all of the futures and wake up on the first one that's ready
            let select = Select::new(rx_ready, tx_ready, wakeups, timer_ready);

            let select = cooldown.wrap(select);

            let select::Outcome {
                rx_result,
                tx_result,
                timeout_expired,
                application_wakeup,
            } = if let Ok(outcome) = select.await {
                outcome
            } else {
                // The endpoint has shut down; stop the event loop
                return;
            };

            // notify the application that we woke up and why
            let wakeup_timestamp = clock.get_time();
            publisher!(wakeup_timestamp).on_platform_event_loop_wakeup(
                event::builder::PlatformEventLoopWakeup {
                    timeout_expired,
                    rx_ready: rx_result.is_some(),
                    tx_ready: tx_result.is_some(),
                    application_wakeup,
                },
            );

            match rx_result {
                Some(Ok(())) => {
                    // we received some packets. give them to the endpoint.
                    rx.queue(|queue| {
                        endpoint.receive(queue, &clock);
                    });
                }
                Some(Err(error)) => {
                    // The RX provider has encountered an error. shut down the event loop
                    let mut publisher = publisher!(clock.get_time());
                    rx.handle_error(error, &mut publisher);
                    return;
                }
                None => {
                    // We didn't receive any packets; nothing to do
                }
            }

            match tx_result {
                Some(Ok(())) => {
                    // The TX queue was full and now has capacity. The endpoint can now continue to
                    // transmit
                }
                Some(Err(error)) => {
                    // The RX provider has encountered an error. shut down the event loop
                    let mut publisher = publisher!(clock.get_time());
                    tx.handle_error(error, &mut publisher);
                    return;
                }
                None => {
                    // The TX queue is either waiting to be flushed or has capacity. Either way, we
                    // call `endpoint.transmit` to at least update the clock and poll any timer
                    // expirations.
                }
            }

            // Let the endpoint transmit, if possible
            tx.queue(|queue| {
                endpoint.transmit(queue, &clock);
            });

            // Get the next expiration from the endpoint and update the timer
            let timeout = endpoint.timeout();
            if let Some(timeout) = timeout {
                timer.update(timeout);
            }

            let sleep_timestamp = clock.get_time();
            // compute the relative timeout to the current time
            let timeout = timeout.map(|t| t.saturating_duration_since(sleep_timestamp));
            // compute how long it took to process the current iteration
            let processing_duration = sleep_timestamp.saturating_duration_since(wakeup_timestamp);

            // publish the event to the application
            publisher!(sleep_timestamp).on_platform_event_loop_sleep(
                event::builder::PlatformEventLoopSleep {
                    timeout,
                    processing_duration,
                },
            );
        }
    }
}
