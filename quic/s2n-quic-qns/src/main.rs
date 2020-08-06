use std::io;
use structopt::StructOpt;

mod client;
mod endpoint;
mod server;
mod socket;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    Arguments::from_args().run().await
}

#[derive(Debug, StructOpt)]
enum Arguments {
    Interop(Interop),
}

impl Arguments {
    pub async fn run(&self) -> io::Result<()> {
        match self {
            Self::Interop(interop) => interop.run().await,
        }
    }
}

#[derive(Debug, StructOpt)]
enum Interop {
    Server(server::Interop),
    Client(client::Interop),
}

impl Interop {
    pub async fn run(&self) -> io::Result<()> {
        match self {
            Self::Server(server) => server.run().await,
            Self::Client(client) => client.run().await,
        }
    }
}
