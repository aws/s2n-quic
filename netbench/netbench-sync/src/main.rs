#![allow(unused_imports)]

use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    join,
    net::{TcpListener, TcpStream},
    spawn,
    task::JoinHandle,
    time::{sleep, timeout, Duration},
    try_join,
};

use std::io::ErrorKind;
use std::fs::File;
use std::process::Stdio;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use structopt::StructOpt;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Endpoint {
    Client,
    Server,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "Coordinate Netbench runs")]
enum Opt {
    Server {
        #[structopt(long, short)]
        server_location: String,
        #[structopt(long, short)]
        client_location: String,
        command_args: Vec<String>,
    },
    Client {
        #[structopt(long, short)]
        server_location: String,
        #[structopt(long, short)]
        client_location: String,
        command_args: Vec<String>,
    },
}

impl Opt {
    fn endpoint_type(&self) -> Endpoint {
        match self {
            Opt::Server { .. } => Endpoint::Server,
            Opt::Client { .. } => Endpoint::Client,
        }
    }

    fn command_args(self) -> Vec<String> {
        match self {
            Opt::Server { command_args, .. } | Opt::Client { command_args, .. } => command_args,
        }
    }

    fn serve_at(&self) -> String {
        match self {
            Opt::Server{server_location, ..} => server_location,
            Opt::Client{client_location, ..} => client_location,
        }.clone()
    }

    fn poll_at(&self) -> String {
        match self {
            Opt::Server{client_location, ..} => client_location,
            Opt::Client{server_location, ..} => server_location,
        }.clone()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
enum State {
    NotReady,
    Ready,
    Running,
    Finished,
}

impl State {
    fn as_u8(&self) -> u8 {
        match self {
            State::NotReady => 0,
            State::Ready => 1,
            State::Running => 2,
            State::Finished => 3,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => State::NotReady,
            1 => State::Ready,
            2 => State::Running,
            3 => State::Finished,
            _ => panic!("Bad value"),
        }
    }

    fn as_atomic(&self) -> Arc<AtomicU8> {
        Arc::new(AtomicU8::new(self.as_u8()))
    }

    fn load(atomic: &Arc<AtomicU8>) -> Self {
        Self::from_u8(atomic.load(Ordering::Relaxed))
    }

    fn store(&self, atomic: &Arc<AtomicU8>) {
        atomic.store(self.as_u8(), Ordering::Relaxed);
    }
}

async fn serve_status(
    status: Arc<AtomicU8>,
    at_location: &str,
) -> io::Result<JoinHandle<io::Result<()>>> {
    let listener = TcpListener::bind(at_location).await?;
    Ok(tokio::spawn(async move {
        loop {
            let state = State::load(&status);
            if state == State::Finished {
                break Err(io::Error::new(ErrorKind::Other, "Finished"));
            }
            let (mut socket, _) = match timeout(Duration::from_secs(5), listener.accept()).await {
                Ok(Ok(o)) => o,
                Ok(Err(_)) => continue,
                Err(_) => continue,
            };
            socket.write_all(&[State::load(&status).as_u8()]).await?;
        }
    }))
}

fn main_work_of_a_client(status: Arc<AtomicU8>, opt: Opt) -> JoinHandle<io::Result<()>> {
    tokio::spawn(async move {
        State::NotReady.store(&status);
        println!("Client Status: Not Ready - Compiling Ect...");
        State::Ready.store(&status);
        println!("Client Status: Ready");
        join!(poll_location(
            opt.poll_at(),
            State::Running,
            State::NotReady
        ))
        .0??;
        State::Running.store(&status);
        println!("Client Status: Running");
        join!(work(opt)).0??;
        State::Finished.store(&status);
        println!("Client Status: Finished");
        Err(io::Error::new(ErrorKind::Other, "Finished"))
    })
}

fn main_work_of_a_server(status: Arc<AtomicU8>, opt: Opt) -> JoinHandle<io::Result<()>> {
    tokio::spawn(async move {
        State::NotReady.store(&status);
        println!("Server Status: Not Ready");
        State::Ready.store(&status);
        join!(poll_location(
            opt.poll_at(),
            State::Ready,
            State::NotReady
        ))
        .0??;
        println!("Server Status: Ready");
        State::Running.store(&status);
        println!("Server Status: Running!");
        let (_, child) = try_join!(
            poll_location(opt.poll_at(), State::Finished, State::Finished),
            work(opt)
        )?;
        child?.kill()?;
        State::Finished.store(&status);
        println!("Server Status: Finished!");
        Err(io::Error::new(ErrorKind::Other, "Finished"))
    })
}

fn poll_location(
    location: String,
    state: State,
    assume_on_no_response: State,
) -> JoinHandle<io::Result<()>> {
    tokio::spawn(async move {
        sleep(Duration::from_secs(0)).await; // Initial Delay
        loop {
            let client_state = get_state_from_location(&location)
                .await
                .unwrap_or(assume_on_no_response.clone());
            if client_state != state {
                sleep(Duration::from_secs(5)).await;
            } else {
                break;
            }
        }
        Ok(())
    })
}

async fn get_state_from_location(location: &str) -> Option<State> {
    let mut stream = match TcpStream::connect(location).await {
        Ok(s) => s,
        Err(_) => return None,
    };
    Some(State::from_u8(stream.read_u8().await.unwrap_or(0)))
}

fn work(opt: Opt) -> JoinHandle<io::Result<Child>> {
    tokio::spawn(async move {
        let ca = opt.command_args();
        Command::new(ca[0].clone()).args(&ca[1..]).spawn()
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let opt = Opt::from_args();
    let status = State::as_atomic(&State::NotReady);

    let loc = opt.serve_at();

    let join_status = serve_status(status.clone(), &loc).await?;
    let join_work = if opt.endpoint_type() == Endpoint::Client {
        main_work_of_a_client(status.clone(), opt)
    } else {
        main_work_of_a_server(status.clone(), opt)
    };

    let (_status, _work) = try_join!(join_status, join_work)?;
    Ok(())
}
