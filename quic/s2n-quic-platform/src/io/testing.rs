// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::select::{self, Select};
use bach::time::scheduler;
use core::{pin::Pin, task::Poll};
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::SocketAddress,
};

type Error = std::io::Error;
type Result<T = (), E = Error> = core::result::Result<T, E>;

mod model;
pub mod network;
pub mod time;

pub use model::Model;
pub use network::{Network, PathHandle};
pub use time::now;

pub use bach::task::{self, primary, spawn};

pub mod rand {
    pub use ::bach::rand::*;

    #[derive(Clone, Copy, Default)]
    pub struct Havoc;

    impl s2n_quic_core::havoc::Random for Havoc {
        #[inline]
        fn fill(&mut self, bytes: &mut [u8]) {
            fill_bytes(bytes);
        }

        #[inline]
        fn gen_bool(&mut self) -> bool {
            gen()
        }

        #[inline]
        fn shuffle(&mut self, bytes: &mut [u8]) {
            shuffle(bytes);
        }

        #[inline]
        fn gen_range(&mut self, range: core::ops::Range<usize>) -> usize {
            gen_range(range)
        }
    }
}

pub mod executor {
    pub use bach::executor::{Handle, JoinHandle};
}

pub struct Executor<N: Network> {
    executor: bach::executor::Executor<Env<N>>,
    handle: Handle,
}

impl<N: Network> Executor<N> {
    pub fn new(network: N, seed: u64) -> Self {
        let mut executor = bach::executor::Executor::new(|handle| Env {
            handle: handle.clone(),
            time: scheduler::Scheduler::new(),
            rand: bach::rand::Scope::new(seed),
            buffers: network::Buffers::default(),
            network,
            stalled_iterations: 0,
        });

        let handle = Handle {
            executor: executor.handle().clone(),
            buffers: executor.environment().buffers.clone(),
        };

        Self { executor, handle }
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn enter<F: FnOnce() -> O, O>(&mut self, f: F) -> O {
        self.executor.environment().enter(f)
    }

    pub fn run(&mut self) {
        self.executor.block_on_primary();
    }

    pub fn close(&mut self) {
        // close the environment, which notifies all of the tasks that we're shutting down
        self.executor.environment().close(|| {});
        while self.executor.macrostep() > 0 {}

        // then close the actual executor
        self.executor.close()
    }
}

impl<N: Network> Drop for Executor<N> {
    fn drop(&mut self) {
        self.close();
    }
}

struct Env<N> {
    handle: bach::executor::Handle,
    time: scheduler::Scheduler,
    rand: bach::rand::Scope,
    buffers: network::Buffers,
    network: N,
    stalled_iterations: usize,
}

impl<N> Env<N> {
    fn enter<F: FnOnce() -> O, O>(&self, f: F) -> O {
        self.handle.enter(|| self.time.enter(|| self.rand.enter(f)))
    }

    fn close<F: FnOnce()>(&mut self, f: F) {
        let handle = &mut self.handle;
        let rand = &mut self.rand;
        let time = &mut self.time;
        let buffers = &mut self.buffers;
        handle.enter(|| {
            rand.enter(|| {
                time.close();
                time.enter(|| {
                    buffers.close();
                    f();
                });
            })
        })
    }
}

impl<N: Network> bach::executor::Environment for Env<N> {
    fn run<Tasks, F>(&mut self, tasks: Tasks) -> Poll<()>
    where
        Tasks: Iterator<Item = F> + Send,
        F: 'static + FnOnce() -> Poll<()> + Send,
    {
        let mut is_ready = true;

        let Self {
            handle,
            time,
            rand,
            buffers,
            network,
            ..
        } = self;

        handle.enter(|| {
            time.enter(|| {
                rand.enter(|| {
                    for task in tasks {
                        is_ready &= task().is_ready();
                    }
                    network.execute(buffers);
                })
            })
        });

        if is_ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn on_macrostep(&mut self, count: usize) {
        // only advance time after a stall
        if count > 0 {
            self.stalled_iterations = 0;
            return;
        }

        self.stalled_iterations += 1;

        // A stalled iteration is a macrostep that didn't actually execute any tasks.
        //
        // The idea with limiting it prevents the runtime from looping endlessly and not
        // actually doing any work. The value of 100 was chosen somewhat arbitrarily as a high
        // enough number that we won't get false positives but low enough that the number of
        // loops stays within reasonable ranges.
        if self.stalled_iterations > 100 {
            panic!("the runtime stalled after 100 iterations");
        }

        while let Some(time) = self.time.advance() {
            let _ = time;
            if self.time.wake() > 0 {
                break;
            }
        }
    }

    fn close<F>(&mut self, close: F)
    where
        F: 'static + FnOnce() + Send,
    {
        Self::close(self, close)
    }
}

#[derive(Clone)]
pub struct Handle {
    executor: executor::Handle,
    buffers: network::Buffers,
}

impl Handle {
    pub fn builder(&self) -> Builder {
        Builder {
            handle: self.clone(),
            address: None,
        }
    }
}

pub struct Builder {
    handle: Handle,
    address: Option<SocketAddress>,
}

impl Builder {
    pub fn build(self) -> Result<Io> {
        Ok(Io { builder: self })
    }
}

pub struct Io {
    builder: Builder,
}

impl Io {
    pub fn start<E: Endpoint<PathHandle = network::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<(executor::JoinHandle<()>, SocketAddress)> {
        let Builder {
            handle: Handle { executor, buffers },
            address,
        } = self.builder;

        let handle = address.unwrap_or_else(|| buffers.generate_addr());

        buffers.register(handle);

        let instance = Instance {
            buffers,
            handle,
            endpoint,
        };
        let join = executor.spawn(instance.event_loop());
        Ok((join, handle))
    }
}

struct Instance<E> {
    buffers: network::Buffers,
    handle: SocketAddress,
    endpoint: E,
}

impl<E: Endpoint<PathHandle = network::PathHandle>> Instance<E> {
    async fn event_loop(self) {
        let Self {
            buffers,
            handle,
            mut endpoint,
        } = self;

        let clock = time::Clock::default();
        let mut timer = time::Timer::default();

        loop {
            let io_task = buffers.readiness(handle);

            // make a future that never returns since we have a single future that checks both
            let empty_task = futures::future::pending::<()>();

            let mut wakeups = endpoint.wakeups(&clock);
            let mut wakeups = Pin::new(&mut wakeups);

            let select::Outcome {
                rx_result,
                tx_result,
                timeout_expired,
                application_wakeup,
            } = if let Ok(res) = Select::new(io_task, empty_task, &mut wakeups, &mut timer).await {
                res
            } else {
                // The endpoint has shut down
                return;
            };

            let wakeup_timestamp = time::now();
            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp: wakeup_timestamp,
                },
                None,
                subscriber,
            );

            publisher.on_platform_event_loop_wakeup(event::builder::PlatformEventLoopWakeup {
                timeout_expired,
                rx_ready: rx_result.is_some(),
                tx_ready: tx_result.is_some(),
                application_wakeup,
            });

            if let Some(result) = rx_result {
                if result.is_err() {
                    // the endpoint shut down
                    return;
                }

                buffers.rx(handle, |queue| {
                    endpoint.receive(queue, &clock);
                });
            }

            buffers.tx(handle, |queue| {
                endpoint.transmit(queue, &clock);
            });

            if let Some(timestamp) = endpoint.timeout() {
                timer.update(timestamp);
            } else {
                timer.cancel();
            }
        }
    }
}
