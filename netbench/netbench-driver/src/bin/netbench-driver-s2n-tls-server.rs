use netbench::{multiplex, scenario, Result};
use netbench_driver::Allocator;
use std::{collections::HashSet, sync::Arc};
use std::fs::File;
use std::io::BufReader;
use structopt::StructOpt;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    spawn,
};
use s2n_tls_tokio::{TlsAcceptor, TlsConnector};
use s2n_tls::raw::{
    config::{Builder, Config},
    error::Error,
    security::DEFAULT_TLS13,
};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    Server::from_args().run().await
}

#[derive(Debug, StructOpt)]
pub struct Server {
    #[structopt(flatten)]
    opts: netbench_driver::Server,
}

impl Server {
    pub async fn run(&self) -> Result<()> {
        let scenario = self.opts.scenario();

        let server = self.server().await?;

        let trace = self.opts.trace();
        let acceptor = TlsAcceptor::new(self.config()?.build()?);
        let acceptor: s2n_tls_tokio::TlsAcceptor = acceptor.into();
        let acceptor = Arc::new(acceptor);

        let config = netbench::multiplex::Config::default();

        let mut conn_id = 0;
        loop {
            let (connection, _addr) = server.accept().await?;
            let scenario = scenario.clone();
            let id = conn_id;
            conn_id += 1;
            let acceptor = acceptor.clone();
            let trace = trace.clone();
            let config = config.clone();
            spawn(async move {
                if let Err(err) = handle_connection(
                    acceptor,
                    connection,
                    id,
                    scenario,
                    trace,
                    config
                ).await {
                    eprintln!("error: {}", err);
                }
            });
        }

        async fn handle_connection(
            acceptor: Arc<s2n_tls_tokio::TlsAcceptor>,
            connection: TcpStream,
            conn_id: u64,
            scenario: Arc<scenario::Server>,
            mut trace: impl netbench::Trace,
            config: multiplex::Config,
        ) -> Result<()> {
            let mut connection = acceptor.accept(connection).await?;
            let server_idx = connection.read_u64().await?;
            let scenario = scenario
                .connections
                .get(server_idx as usize)
                .ok_or("invalid connection id")?;
            let connection = Box::pin(connection);

            let conn = netbench::Driver::new(
                scenario,
                netbench::multiplex::Connection::new(
                    conn_id,
                    connection,
                    config
                )
            );

            let mut checkpoints = HashSet::new();
            let mut timer = netbench::timer::Tokio::default();

            conn.run(&mut trace, &mut checkpoints, &mut timer).await?;

            Ok(())
        }
    }

    async fn server(&self) -> Result<TcpListener> {
        let server = TcpListener::bind((self.opts.ip, self.opts.port)).await?;
        Ok(server)
    }

    fn config(&self) -> Result<Builder, Error> {
        let (cert, private_key) = self.opts.certificate();

        let mut builder = Config::builder();
        builder.set_security_policy(&DEFAULT_TLS13)?;
        builder.load_pem(cert.pem.as_bytes(), private_key.pem.as_bytes())?;

        Ok(builder)
    }
}
