use std::io;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Interop {}

impl Interop {
    pub async fn run(&self) -> io::Result<()> {
        eprintln!("unsupported");
        std::process::exit(127);
    }
}
