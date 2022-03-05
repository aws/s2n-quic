// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    driver::timer,
    operation as ops,
    scenario::{self, Scenario},
    timer::Timer,
    Checkpoints, Result, Trace,
};
use std::{
    collections::BTreeMap,
    future::Future,
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
};

mod thread;

pub trait Client<'a> {
    type Connect: Future<Output = Result<Self::Connection>> + Unpin;
    type Connection: Connection;

    fn connect(
        &mut self,
        addr: SocketAddr,
        hostname: &str,
        server_conn_id: u64,
        ops: &'a Arc<scenario::Connection>,
    ) -> Self::Connect;
}

pub trait Connection: crate::driver::timer::Provider {
    fn poll<T: Trace, Ch: Checkpoints>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: crate::driver::timer::Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>>;
}

pub struct Driver<'a, C: Client<'a>> {
    client: C,
    thread: thread::Thread<'a, C>,
    addresses: &'a AddressMap,
}

impl<'a, C: Client<'a>> Driver<'a, C> {
    pub fn new(client: C, scenario: &'a scenario::Client, addresses: &'a AddressMap) -> Self {
        Self {
            client,
            thread: thread::Thread::new(scenario, &scenario.scenario),
            addresses,
        }
    }

    pub async fn run<T: Trace, Ch: Checkpoints, Ti: Timer>(
        mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        timer: &mut Ti,
    ) -> Result<C> {
        futures::future::poll_fn(|cx| self.poll_with_timer(trace, checkpoints, timer, cx)).await?;
        Ok(self.client)
    }

    pub fn poll_with_timer<T: Trace, Ch: Checkpoints, Ti: Timer>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        timer: &mut Ti,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        let now = timer.now();
        let res = self.poll(trace, checkpoints, now, cx);

        if let Some(target) = timer::Provider::next_expiration(&self) {
            // update the timer with the next expiration
            let _ = timer.poll(target, cx);
        };

        res
    }

    pub fn poll<T: Trace, Ch: Checkpoints>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: timer::Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        self.thread.poll(
            &mut self.client,
            self.addresses,
            trace,
            checkpoints,
            now,
            cx,
        )
    }
}

impl<'a, C: Client<'a>> timer::Provider for Driver<'a, C> {
    fn timers<Q>(&self, query: &mut Q) -> timer::Result
    where
        Q: timer::Query,
    {
        self.thread.timers(query)
    }
}

pub struct AddressMap {
    routers: Vec<BTreeMap<u64, SocketAddr>>,
    servers: Vec<SocketAddr>,
    hostnames: Vec<String>,
}

pub trait Resolver {
    fn server(&mut self, server_id: u64) -> Result<String>;
    fn router(&mut self, router_id: u64, server_id: u64) -> Result<String>;
}

async fn resolve_routes<R: Resolver>(
    id: u64,
    ops: &[ops::Client],
    routes: &mut BTreeMap<u64, SocketAddr>,
    resolver: &mut R,
) -> Result<()> {
    let mut pending: Vec<_> = ops.iter().collect();

    while let Some(op) = pending.pop() {
        match op {
            ops::Client::Connect {
                server_id,
                router_id,
                ..
            } if Some(id) == *router_id => {
                let host = resolver.router(id, *server_id)?;
                let mut addr = tokio::net::lookup_host(host).await?;
                let addr = addr.next().ok_or("invalid address")?;
                routes.insert(*server_id, addr);
            }
            ops::Client::Scope { threads } => {
                for thread in threads {
                    pending.extend(thread);
                }
            }
            _ => {}
        }
    }

    Ok(())
}

impl AddressMap {
    pub async fn new<R: Resolver>(
        scenario: &Scenario,
        client_id: u64,
        resolver: &mut R,
    ) -> Result<Self> {
        let client = &scenario.clients[client_id as usize];

        let mut servers = vec![];
        for id in 0..scenario.servers.len() {
            let host = resolver.server(id as u64)?;
            let mut addr = tokio::net::lookup_host(host).await?;
            let addr = addr.next().ok_or("invalid address")?;
            servers.push(addr);
        }

        let mut routers = vec![];
        for router_id in 0..scenario.routers.len() {
            let mut routes = BTreeMap::new();

            resolve_routes(router_id as u64, &client.scenario, &mut routes, resolver).await?;

            routers.push(routes);
        }

        let hostname_len = scenario
            .servers
            .iter()
            .map(|server| server.connections.len())
            .max()
            .unwrap_or(0);

        let mut hostnames = vec![];
        for idx in 0..hostname_len {
            hostnames.push(format!("{}.{}.net", idx, scenario.id));
        }

        Ok(Self {
            servers,
            routers,
            hostnames,
        })
    }

    pub fn server(&self, server_id: u64) -> SocketAddr {
        self.servers[server_id as usize]
    }

    pub fn router(&self, router_id: u64, server_id: u64) -> SocketAddr {
        *self.routers[router_id as usize]
            .get(&server_id)
            .expect("missing server route")
    }

    pub fn hostname(&self, connection_id: u64) -> &str {
        &self.hostnames[connection_id as usize]
    }
}
