// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use netbench::{scenario::Scenario, units::parse_duration, Result};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json;

use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    join,
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{sleep, timeout},
    try_join,
};

mod bpftrace;
mod generic;
mod procinfo;

use crate::bpftrace::BpftraceHandle;
use crate::generic::GenericHandle;

#[derive(Debug, StructOpt)]
pub struct Args {
    pub driver: String,

    #[structopt(long, short, env = "SCENARIO")]
    pub scenario: String,

    #[structopt(long, short, parse(try_from_str=parse_duration), default_value = "1s")]
    pub interval: Duration,

    #[structopt(long, short)]
    pub coordinate: bool,

    #[structopt(long, required_if("coordinate", "true"))]
    pub server_location: Option<String>,

    #[structopt(long, required_if("coordinate", "true"))]
    pub client_location: Option<String>,

    #[structopt(long, required_if("coordinate", "true"))]
    pub run_as: Option<String>,

    #[structopt(long, short)]
    pub verbose: bool,
}

impl Args {
    pub fn scenario(&self) -> Result<Scenario> {
        Scenario::open(std::path::Path::new(&self.scenario))
    }
    pub fn as_server(&self) -> Option<bool> {
        self.run_as.as_ref().map(|s| s.eq("server".into()))
    }
    pub fn as_client(&self) -> Option<bool> {
        self.run_as.as_ref().map(|s| s.eq("client".into()))
    }

    pub fn location(&self) -> Option<String> {
        if self.as_server()? {
            self.server_location.clone()
        } else if self.as_client()? {
            self.client_location.clone()
        } else {
            unimplemented!("Only --run-as server and --run-as client are supported options");
        }
    }
    pub fn other_location(&self) -> Option<String> {
        if self.as_server()? {
            self.client_location.clone()
        } else if self.as_client()? {
            self.server_location.clone()
        } else {
            unimplemented!("Only --run-as server and --run-as client are supported options");
        }
    }
}

pub trait RunHandle {
    fn wait(self) -> Result<()>;
    fn kill(self) -> Result<()>;
}

enum Handle {
    Generic(GenericHandle),
    Bpf(BpftraceHandle),
}

impl RunHandle for Handle {
    fn wait(self) -> Result<()> {
        match self {
            Self::Generic(h) => h.wait(),
            Self::Bpf(h) => h.wait(),
        }
    }
    fn kill(self) -> Result<()> {
        match self {
            Self::Generic(h) => h.kill(),
            Self::Bpf(h) => h.kill(),
        }
    }
}

fn run(args: Args) -> JoinHandle<Handle> {
    tokio::spawn(async move {
        // try to use bpftrace
        if let Some(trace_handle) = bpftrace::try_run(&args).unwrap() {
            return Handle::Bpf(trace_handle);
        }

        // fall back to the generic collector
        Handle::Generic(generic::run(&args).unwrap())
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
enum State {
    NotReady,
    Ready,
    Running,
    Finished,
}

#[derive(Serialize, Deserialize)]
struct StateMessage {
    version: String,
    state: State,
}

impl From<State> for StateMessage {
    fn from(value: State) -> StateMessage {
        Self {
            version: "18-04-2023".to_string(),
            state: value,
        }
    }
}

impl TryFrom<u8> for State {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(State::NotReady),
            1 => Ok(State::Ready),
            2 => Ok(State::Running),
            3 => Ok(State::Finished),
            _ => Err(()),
        }
    }
}

impl From<State> for u8 {
    fn from(value: State) -> u8 {
        match value {
            State::NotReady => 0,
            State::Ready => 1,
            State::Running => 2,
            State::Finished => 3,
        }
    }
}

#[derive(Debug, Clone)]
struct StateTracker {
    current_state: Arc<AtomicU8>,
    location: String,
    other_location: String,
    verbose: bool,
}

macro_rules! log_polling {
    ($verbose:ident, $($arg:tt)*) => {
        if $verbose {
            println!("[POLLING] {}", format!($($arg)*));
        }
    }
}

macro_rules! log_serving {
    ($verbose:ident, $($arg:tt)*) => {
        if $verbose {
            println!("[SERVING] {}", format!($($arg)*));
        }
    }
}

impl StateTracker {
    fn store(&mut self, value: State) {
        if self.verbose {
            println!("[LOCAL STATE] Updating local state to {:?}", value);
        }
        self.current_state.store(value.into(), Ordering::Relaxed)
    }
    fn new(location: String, other_location: String, verbose: bool) -> Self {
        Self {
            current_state: Arc::new(AtomicU8::new(State::NotReady.into())),
            other_location,
            location,
            verbose,
        }
    }
    async fn get_state(
        from_location: String,
        assume_on_no_response: State,
        verbose: bool,
    ) -> State {
        let mut stream = match TcpStream::connect(from_location.as_str()).await {
            Ok(stream) => stream,
            Err(_) => return assume_on_no_response,
        };
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await.expect("Failed to read from peer?");
        let option_other_state = serde_json::from_slice(&buffer).ok().map(|sm: StateMessage| sm.state);
        if let Some(other_state) = option_other_state {
            log_polling!(
                verbose,
                "{} reports state: {:?}",
                from_location,
                other_state
            );
            other_state
        } else {
            log_polling!(
                verbose,
                "{} unavailable, assuming state: {:?}",
                from_location,
                assume_on_no_response
            );
            assume_on_no_response
        }
    }
    fn poll(
        &self,
        wait_for: State,
        assume_on_no_response: State,
        initial_delay: Duration,
        poll_delay: Duration,
    ) -> JoinHandle<io::Result<()>> {
        let other_location = self.other_location.clone();
        let verbose = self.verbose;
        tokio::spawn(async move {
            log_polling!(
                verbose,
                "Initial Delay Sleeping for {} seconds",
                initial_delay.as_secs()
            );
            sleep(initial_delay).await; // Initial Delay
            loop {
                let new_state =
                    Self::get_state(other_location.clone(), assume_on_no_response, verbose).await;
                if new_state == wait_for {
                    break;
                }
                log_polling!(verbose, "Delaying for {} seconds", poll_delay.as_secs());
                sleep(poll_delay).await;
            }
            Ok(())
        })
    }
    async fn serve(&self) -> Result<JoinHandle<io::Result<()>>> {
        let listener = TcpListener::bind(self.location.as_str()).await?;
        let current_state = self.current_state.clone();
        let verbose = self.verbose;
        Ok(tokio::spawn(async move {
            loop {
                let state: State = current_state
                    .clone()
                    .load(Ordering::Relaxed)
                    .try_into()
                    .expect("An invalid atomic u8 got constructed.");
                if state == State::Finished {
                    log_serving!(verbose, "We are done; stop serving state");
                    break Err(io::Error::new(ErrorKind::Other, "Finished"));
                }
                let (mut socket, _) = match timeout(Duration::from_secs(5), listener.accept()).await
                {
                    Ok(Ok(o)) => o,
                    _ => {
                        log_serving!(verbose, "No peers made connection in the last 5 seconds; checking for finished status");
                        continue;
                    }
                };
                let served_state: State = current_state
                    .load(Ordering::Relaxed)
                    .try_into()
                    .expect("An invalid atomic u8 got constructed.");
                socket.write_all(&serde_json::to_vec(&StateMessage::from(served_state)).unwrap()).await?;
                log_serving!(verbose, "Told peer we are in state {:?}", served_state);
            }
        }))
    }
}

macro_rules! log_server {
    ($verbose:ident, $($arg:tt)*) => {
        if $verbose {
            println!("[SERVER] {}", format!($($arg)*));
        }
    }
}

macro_rules! log_client {
    ($verbose:ident, $($arg:tt)*) => {
        if $verbose {
            println!("[CLIENT] {}", format!($($arg)*));
        }
    }
}

fn server_state_machine(args: Args, mut state_tracker: StateTracker) -> JoinHandle<io::Result<()>> {
    let verbose = state_tracker.verbose;
    tokio::spawn(async move {
        state_tracker.store(State::Ready);
        log_server!(verbose, "Ready to go; waiting to Client to report Ready.");
        join!(state_tracker.poll(
            State::Ready,
            State::NotReady,
            Duration::from_secs(5),
            Duration::from_secs(5)
        ))
        .0
        .unwrap()
        .unwrap();
        log_server!(verbose, "Client is ready; Starting Server");
        state_tracker.store(State::Running);
        let (poll, child) = try_join!(
            state_tracker.poll(
                State::Finished,
                State::Finished,
                Duration::from_secs(1),
                Duration::from_secs(5)
            ),
            run(args)
        )?;
        poll?;
        log_server!(verbose, "Client is Finished; We need to kill the server.");
        child.kill().expect("Failed to kill child?");
        log_server!(verbose, "Finished");
        state_tracker.store(State::Finished);
        Err(io::Error::new(ErrorKind::Other, String::from("Finished")))
    })
}

fn client_state_machine(args: Args, mut state_tracker: StateTracker) -> JoinHandle<io::Result<()>> {
    let verbose = state_tracker.verbose;
    tokio::spawn(async move {
        state_tracker.store(State::Ready);
        log_client!(verbose, "Ready to go; waiting for Server to start running.");
        join!(state_tracker.poll(
            State::Running,
            State::NotReady,
            Duration::from_secs(5),
            Duration::from_secs(5)
        ))
        .0??;
        log_client!(verbose, "Server is running; Starting Client.");
        state_tracker.store(State::Running);
        let handle = join!(run(args)).0?;
        handle.wait().unwrap();
        log_client!(verbose, "Finished.");
        state_tracker.store(State::Finished);
        Err(io::Error::new(ErrorKind::Other, "Finished"))
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::from_args();
    if args.coordinate {
        let state_tracker = StateTracker::new(
            args.location().unwrap(),
            args.other_location().unwrap(),
            args.verbose,
        );
        let state_server = state_tracker.serve().await?;
        let state_machine = if let Some(true) = args.as_server() {
            server_state_machine(args, state_tracker)
        } else if let Some(true) = args.as_client() {
            client_state_machine(args, state_tracker)
        } else {
            unimplemented!("Only --run-as client and --run-as server are supported.")
        };
        let (_, _) = try_join!(state_server, state_machine)?;
        Ok(())
    } else {
        let run_handle = run(args).await?;
        run_handle.wait()?;
        Ok(())
    }
}
