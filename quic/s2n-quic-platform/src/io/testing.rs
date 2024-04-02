// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bach::time::scheduler;
use core::task::Poll;
use s2n_quic_core::{
    endpoint::Endpoint, inet::SocketAddress, io::event_loop::EventLoop, path::mtu,
};

type Error = std::io::Error;
type Result<T = (), E = Error> = core::result::Result<T, E>;

pub mod message;
mod model;
pub mod network;
mod socket;
pub mod time;

pub use model::{Model, TxRecorder};
pub use network::{Network, PathHandle};
pub use socket::Socket;
pub use time::now;

pub use bach::task::{self, primary, spawn};

// returns `true` if the caller is being executed in a testing environment
pub fn is_in_env() -> bool {
    bach::task::scope::try_borrow_with(|scope| scope.is_some())
}

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
        fn gen_range(&mut self, range: core::ops::Range<u64>) -> u64 {
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
                // if a task has woken, then reset the stall count
                self.stalled_iterations = 0;
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
            on_socket: None,
            mtu_config_builder: mtu::Config::builder(),
            queue_recv_buffer_size: None,
            queue_send_buffer_size: None,
        }
    }
}

pub struct Builder {
    handle: Handle,
    address: Option<SocketAddress>,
    on_socket: Option<Box<dyn FnOnce(socket::Socket)>>,
    mtu_config_builder: mtu::Builder,
    queue_recv_buffer_size: Option<u32>,
    queue_send_buffer_size: Option<u32>,
}

impl Builder {
    pub fn build(self) -> Result<Io> {
        Ok(Io { builder: self })
    }

    pub fn with_base_mtu(mut self, base_mtu: u16) -> Self {
        self.mtu_config_builder = self.mtu_config_builder.with_base_mtu(base_mtu).unwrap();
        self
    }

    pub fn with_initial_mtu(mut self, initial_mtu: u16) -> Self {
        self.mtu_config_builder = self
            .mtu_config_builder
            .with_initial_mtu(initial_mtu)
            .unwrap();
        self
    }

    pub fn with_max_mtu(mut self, max_mtu: u16) -> Self {
        self.mtu_config_builder = self.mtu_config_builder.with_max_mtu(max_mtu).unwrap();
        self
    }

    pub fn on_socket(mut self, f: impl FnOnce(socket::Socket) + 'static) -> Self {
        self.on_socket = Some(Box::new(f));
        self
    }

    /// Sets the size of the send buffer associated with the transmit side (internal to s2n-quic)
    pub fn with_internal_send_buffer_size(
        mut self,
        send_buffer_size: usize,
    ) -> std::io::Result<Self> {
        self.queue_send_buffer_size = Some(send_buffer_size.try_into().map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{err}"))
        })?);
        Ok(self)
    }

    /// Sets the size of the send buffer associated with the receive side (internal to s2n-quic)
    pub fn with_internal_recv_buffer_size(
        mut self,
        recv_buffer_size: usize,
    ) -> std::io::Result<Self> {
        self.queue_recv_buffer_size = Some(recv_buffer_size.try_into().map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{err}"))
        })?);
        Ok(self)
    }
}

pub struct Io {
    builder: Builder,
}

impl Io {
    /// Returns a socket handle for the Io endpoint
    pub fn socket(self) -> Socket {
        let Builder {
            handle: Handle {
                executor: _,
                buffers,
            },
            address,
            on_socket,
            mtu_config_builder,
            queue_recv_buffer_size: _,
            queue_send_buffer_size: _,
        } = self.builder;

        let handle = address.unwrap_or_else(|| buffers.generate_addr());

        let socket = buffers.register(handle, mtu_config_builder.build().unwrap().max_mtu);

        if let Some(on_socket) = on_socket {
            on_socket(socket.clone());
        }

        socket
    }

    pub fn start<E: Endpoint<PathHandle = network::PathHandle>>(
        self,
        mut endpoint: E,
    ) -> Result<(executor::JoinHandle<()>, SocketAddress)> {
        let Builder {
            handle: Handle { executor, buffers },
            address,
            on_socket,
            mtu_config_builder,
            queue_recv_buffer_size,
            queue_send_buffer_size,
        } = self.builder;
        let mtu_config = mtu_config_builder.build().unwrap();
        endpoint.set_mtu_config(mtu_config);

        let handle = address.unwrap_or_else(|| buffers.generate_addr());

        let socket = buffers.register(handle, mtu_config.max_mtu);
        let tx = socket.tx_task(mtu_config.max_mtu, queue_send_buffer_size);
        let rx = socket.rx_task(mtu_config.max_mtu, queue_recv_buffer_size);

        if let Some(on_socket) = on_socket {
            on_socket(socket);
        }

        let clock = time::Clock::default();

        let event_loop = EventLoop {
            endpoint,
            clock,
            tx,
            rx,
            cooldown: Default::default(),
        };
        let join = executor.spawn(event_loop.start());
        Ok((join, handle))
    }
}
