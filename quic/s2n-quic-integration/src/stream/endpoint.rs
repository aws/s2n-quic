use crate::{
    api,
    stream::scenario::{Scenario, Streams},
};
use anyhow::Result;
use core::future::Future;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Instructions {
    pub client: Arc<Streams>,
    pub server: Arc<Streams>,
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
    local: Arc<Streams>,
    peer: Arc<Streams>,
}

impl Endpoint {
    fn run<C: 'static + api::Connection>(
        &self,
        connection: C,
    ) -> impl Future<Output = Result<()>> + 'static {
        let (handle, acceptor) = connection.split();
        let local = Self::local(&self.local, handle);
        let peer = Self::peer(&self.peer, acceptor);
        async move { futures::try_join!(local, peer).map(|_| ()) }
    }

    fn local<H: api::Handle>(
        _streams: &Streams,
        _handle: H,
    ) -> impl Future<Output = Result<()>> + 'static {
        // TODO implement me
        async { todo!() }
    }

    fn peer<A: api::Acceptor>(
        _streams: &Streams,
        _acceptor: A,
    ) -> impl Future<Output = Result<()>> + 'static {
        // TODO implement me
        async { todo!() }
    }
}
