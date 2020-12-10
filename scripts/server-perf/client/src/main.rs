use anyhow::{Context, Result};
use bytesize::ByteSize;
use std::time::Instant;
use structopt::StructOpt;
use url::Url;

static CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../quic/s2n-quic-qns/certs/cert.der"
));

#[derive(Debug, StructOpt)]
struct Args {
    url: Url,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cert = quinn::Certificate::from_der(CERT).unwrap();

    let args = Args::from_args();

    client(args.url, cert).await
}

async fn client(url: Url, server_cert: quinn::Certificate) -> Result<()> {
    let hostname = url.host_str().expect("missing hostname");
    let server_addr = url.socket_addrs(|| Some(4433))?[0];

    let mut endpoint = quinn::Endpoint::builder();
    let mut client_config = quinn::ClientConfigBuilder::default();
    client_config.protocols(&["hq-29".as_bytes()]);
    client_config.add_certificate_authority(server_cert)?;
    endpoint.default_client_config(client_config.build());

    let (endpoint, _) = endpoint.bind(&"[::]:0".parse().unwrap())?;

    let quinn::NewConnection { connection, .. } = endpoint
        .connect(&server_addr, hostname)?
        .await
        .context("unable to connect")?;

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .context("failed to open stream")?;

    // make the request
    let request = format!("GET {}\r\n", url.path());
    send.write_all(request.as_bytes()).await?;
    send.finish().await.context("failed finishing stream")?;

    // record the response
    let start = Instant::now();

    let mut recv_len = 0usize;
    let mut buf = [0u8; 10_000];
    while let Some(len) = recv.read(&mut buf).await? {
        recv_len += len;
    }
    let duration = start.elapsed();
    let bytes_per_sec = (recv_len as f64) / duration.as_secs_f64();

    eprintln!(
        "response received in {:?} - {}/s",
        duration,
        ByteSize::b(bytes_per_sec as u64)
    );

    Ok(())
}
