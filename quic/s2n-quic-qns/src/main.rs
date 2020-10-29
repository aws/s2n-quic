use structopt::StructOpt;

pub type Error = Box<dyn 'static + std::error::Error + Send + Sync>;
pub type Result<T> = core::result::Result<T, Error>;
mod client;
mod server;

#[tokio::main]
async fn main() -> Result<()> {
    Arguments::from_args().run().await
}

#[derive(Debug, StructOpt)]
enum Arguments {
    Interop(Interop),
}

impl Arguments {
    pub async fn run(&self) -> Result<()> {
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
    pub async fn run(&self) -> Result<()> {
        match self {
            Self::Server(server) => server.run().await,
            Self::Client(client) => client.run().await,
        }
    }
}
