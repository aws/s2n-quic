// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    api::{self, BidirectionalStream as _},
    rt::{delay, spawn},
    stream::scenario::{self, Scenario},
};
use anyhow::Result;
use bytes::Bytes;
use core::future::Future;
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Instructions {
    pub client: Arc<scenario::Streams>,
    pub server: Arc<scenario::Streams>,
}

impl Instructions {
    pub fn server(&self) -> Server {
        self.into()
    }

    pub fn client(&self) -> Client {
        self.into()
    }
}

impl From<Scenario> for Instructions {
    fn from(scenario: Scenario) -> Self {
        Self {
            client: Arc::new(scenario.client),
            server: Arc::new(scenario.server),
        }
    }
}

impl From<&Scenario> for Instructions {
    fn from(scenario: &Scenario) -> Self {
        Self {
            client: Arc::new(scenario.client.clone()),
            server: Arc::new(scenario.server.clone()),
        }
    }
}

macro_rules! endpoint {
    ($name:ident, $local:ident, $peer:ident) => {
        #[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
        pub struct $name(Endpoint);

        impl $name {
            pub fn run<C: 'static + api::Connection>(
                &self,
                connection: C,
            ) -> impl Future<Output = Result<()>> + 'static {
                self.0.run(connection)
            }
        }

        impl From<&Scenario> for $name {
            fn from(scenario: &Scenario) -> Self {
                let instructions: Instructions = scenario.into();
                instructions.into()
            }
        }

        impl From<Instructions> for $name {
            fn from(scenario: Instructions) -> Self {
                Self(Endpoint {
                    local: scenario.$local,
                    peer: scenario.$peer,
                })
            }
        }

        impl From<&Instructions> for $name {
            fn from(scenario: &Instructions) -> Self {
                Self(Endpoint {
                    local: scenario.$local.clone(),
                    peer: scenario.$peer.clone(),
                })
            }
        }
    };
}

endpoint!(Server, server, client);
endpoint!(Client, client, server);

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
struct Endpoint {
    local: Arc<scenario::Streams>,
    peer: Arc<scenario::Streams>,
}

impl Endpoint {
    fn run<C: 'static + api::Connection>(
        &self,
        connection: C,
    ) -> impl Future<Output = Result<()>> + 'static {
        let (handle, acceptor) = connection.split();
        let local = Self::local(&self.local, handle);
        let peer = Self::peer(&self.peer, acceptor);
        async move {
            // TODO check if we are allowed to have an error in this test
            let _ = futures::try_join!(local, peer).map(|_| ());
            Ok(())
        }
    }

    fn local<H: 'static + api::Handle>(
        streams: &Arc<scenario::Streams>,
        mut handle: H,
    ) -> impl Future<Output = Result<()>> + 'static {
        let mut handles = vec![];

        for (id, scenario) in streams.uni_streams.iter() {
            let mut handle = handle.clone();
            // Notify the peer of the scenario
            let id = Bytes::copy_from_slice(&id.to_be_bytes());
            let scenario = *scenario;
            handles.push(spawn(async move {
                delay(scenario.delay).await;
                let stream = handle.open_send().await?;
                Self::sender(stream, id, scenario.local).await?;
                <Result<_>>::Ok(())
            }));
        }

        for (id, scenario) in streams.bidi_streams.iter() {
            let mut handle = handle.clone();
            // Notify the peer of the scenario
            let id = Bytes::copy_from_slice(&id.to_be_bytes());
            let scenario = *scenario;
            handles.push(spawn(async move {
                delay(scenario.delay).await;
                let stream = handle.open_bidirectional().await?;
                let (receiver, sender) = stream.split();

                let sender = spawn(Self::sender(sender, id, scenario.local));
                let receiver = spawn(Self::receiver(receiver, Bytes::new(), scenario.peer));
                let (sender, receiver) = futures::try_join!(sender, receiver)?;
                sender?;
                receiver?;
                <Result<_>>::Ok(())
            }));
        }

        async move {
            let results = futures::future::try_join_all(handles).await?;
            for result in results {
                result?;
            }
            handle.close();
            Ok(())
        }
    }

    fn peer<A: 'static + api::Acceptor>(
        scenarios: &Arc<scenario::Streams>,
        acceptor: A,
    ) -> impl Future<Output = Result<()>> + 'static {
        let (bidi, recv) = acceptor.split();

        let recv = spawn(Self::peer_receiver(recv, scenarios.clone()));
        let bidi = spawn(Self::peer_bidirectional(bidi, scenarios.clone()));

        async {
            let (recv, bidi) = futures::try_join!(recv, bidi)?;
            recv?;
            bidi?;
            Ok(())
        }
    }

    async fn peer_receiver<A: 'static + api::ReceiveStreamAcceptor>(
        mut recv: A,
        scenarios: Arc<scenario::Streams>,
    ) -> Result<()> {
        let mut handles = vec![];

        while let Some(mut stream) = recv.accept_receive().await? {
            let scenarios = scenarios.clone();
            handles.push(spawn(async move {
                let (id, prelude) = Self::read_stream_id(&mut stream).await?;

                let scenario = scenarios
                    .uni_streams
                    .get(&id)
                    .unwrap_or_else(|| panic!("missing receive scenario {}", id));

                Self::receiver(stream, prelude, scenario.local).await?;

                <Result<_>>::Ok(id)
            }));
        }

        let mut used: HashSet<u64> = HashSet::new();

        let results = futures::future::try_join_all(handles).await?;
        for result in results {
            let id = result?;

            assert!(used.insert(id), "scenario {} used multiple times", id);
        }

        let complete: HashSet<u64> = scenarios.uni_streams.keys().copied().collect();

        let mut difference: Vec<_> = complete.difference(&used).collect();
        if !difference.is_empty() {
            difference.sort();
            panic!(
                "the following receive scenarios did not occur: {:?}",
                difference
            );
        }

        Ok(())
    }

    async fn peer_bidirectional<A: 'static + api::BidirectionalStreamAcceptor>(
        mut bidi: A,
        scenarios: Arc<scenario::Streams>,
    ) -> Result<()> {
        let mut handles = vec![];

        while let Some(stream) = bidi.accept_bidirectional().await? {
            let scenarios = scenarios.clone();
            handles.push(spawn(async move {
                let (mut receiver, sender) = stream.split();

                let (id, prelude) = Self::read_stream_id(&mut receiver).await?;

                let scenario = scenarios
                    .bidi_streams
                    .get(&id)
                    .unwrap_or_else(|| panic!("missing bidirectional scenario {}", id));

                let sender = spawn(Self::sender(sender, Bytes::new(), scenario.peer));
                let receiver = spawn(Self::receiver(receiver, prelude, scenario.local));
                let (sender, receiver) = futures::try_join!(sender, receiver)?;
                sender?;
                receiver?;

                <Result<_>>::Ok(id)
            }));
        }

        let mut used: HashSet<u64> = HashSet::new();

        let results = futures::future::try_join_all(handles).await?;
        for result in results {
            let id = result?;

            assert!(used.insert(id), "scenario {} used multiple times", id);
        }

        let complete: HashSet<u64> = scenarios.bidi_streams.keys().copied().collect();

        let mut difference: Vec<_> = complete.difference(&used).collect();
        if !difference.is_empty() {
            difference.sort();
            panic!(
                "the following bidirectional scenarios did not occur: {:?}",
                difference
            );
        }

        Ok(())
    }

    async fn sender<S: api::SendStream>(
        mut stream: S,
        prelude: Bytes,
        scenario: scenario::Stream,
    ) -> Result<()> {
        stream.send(prelude).await?;

        let mut sender = scenario.data;
        let mut chunks = [bytes::Bytes::new()];

        let mut send_amount = scenario.send_amount.iter();

        while sender
            .send(send_amount.next().unwrap(), &mut chunks)
            .is_some()
        {
            // TODO implement resets
            stream
                .send(core::mem::replace(&mut chunks[0], Bytes::new()))
                .await?;
        }

        stream.finish().await?;

        Ok(())
    }

    async fn receiver<S: api::ReceiveStream>(
        mut stream: S,
        prelude: Bytes,
        scenario: scenario::Stream,
    ) -> Result<()> {
        let mut receiver = scenario.data;
        receiver.receive(&[prelude]);

        while let Some(chunk) = stream.receive().await? {
            // TODO implement stop_sending
            receiver.receive(&[chunk]);
        }

        assert!(
            receiver.is_finished(),
            "did not receive a complete stream of data from peer"
        );

        Ok(())
    }

    async fn read_stream_id<S: api::ReceiveStream>(stream: &mut S) -> Result<(u64, Bytes)> {
        let mut chunk = Bytes::new();
        let mut offset = 0;
        let mut id = [0u8; core::mem::size_of::<u64>()];

        while offset < id.len() {
            chunk = stream
                .receive()
                .await?
                .expect("every stream should be prefixed with the scenario ID");

            let needed_len = id.len() - offset;
            let len = chunk.len().min(needed_len);

            id[offset..offset + len].copy_from_slice(&chunk[..len]);
            offset += len;
            bytes::Buf::advance(&mut chunk, len);
        }

        let id = u64::from_be_bytes(id);

        Ok((id, chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test::Connection;

    async fn check(scenario: &Scenario) {
        let (client, server) = Connection::pair();

        let scenario: Instructions = scenario.into();

        let client_task = spawn(scenario.client().run(client.clone()));
        let server_task = spawn(scenario.server().run(server.clone()));

        let (client_res, server_res) = futures::try_join!(client_task, server_task).unwrap();
        client_res.unwrap();
        server_res.unwrap();
    }

    #[tokio::test]
    async fn basic_test() {
        check(&Scenario::default()).await;
    }

    #[test]
    fn random_scenario_test() {
        bolero::check!().with_type().for_each(|scenario| {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap()
                .block_on(check(scenario));
        });
    }
}
