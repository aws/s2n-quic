use anyhow::{Context, Result};
use byte_unit::{Byte, ByteUnit};
use bytes::Bytes;
use s2n_quic_core::{crypto::tls::testing::certificates::CERT_DER, stream::testing};
use std::time::Instant;
use structopt::StructOpt;
use url::Url;

#[derive(Debug, StructOpt)]
struct Args {
    url: Url,

    #[structopt(short, long, default_value = "0MB")]
    download: String,

    #[structopt(short, long, default_value = "0MB")]
    upload: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cert = quinn::Certificate::from_der(CERT_DER).unwrap();

    let args = Args::from_args();
    let download = Byte::from_str(&args.download).unwrap();
    let upload = Byte::from_str(&args.upload).unwrap();

    client(args.url, download, upload, cert).await
}

async fn client(
    url: Url,
    download: Byte,
    upload: Byte,
    server_cert: quinn::Certificate,
) -> Result<()> {
    let hostname = url.host_str().expect("missing hostname");
    let server_addr = url.socket_addrs(|| Some(4433))?[0];

    let mut endpoint = quinn::Endpoint::builder();
    let mut client_config = quinn::ClientConfigBuilder::default();
    client_config.protocols(&["perf".as_bytes()]);
    client_config.add_certificate_authority(server_cert)?;
    endpoint.default_client_config(client_config.build());

    let (endpoint, _) = endpoint.bind(&"[::]:0".parse().unwrap())?;

    let quinn::NewConnection { connection, .. } = endpoint
        .connect(&server_addr, hostname)?
        .await
        .context("unable to connect")?;

    let (mut send, recv) = connection
        .open_bi()
        .await
        .context("failed to open stream")?;

    // client is receiving and server is sending
    // here we send the length of bytes we expect the server to send us back
    let dl_len = download.get_bytes() as u64;
    send.write_all(&u64::to_be_bytes(dl_len)).await?;

    // client is sending and server is receiving
    let sender = tokio::spawn(async move { handle_send_stream(send, upload).await });

    // client is receiving and server is sending
    let receiver = tokio::spawn(async move { handle_recv_stream(recv, download).await });

    // record the time
    let all = Instant::now();
    let _ = futures::try_join!(receiver, sender)?;
    let duration = all.elapsed();
    eprintln!("total duration took {:?}", duration,);

    Ok(())
}

async fn handle_recv_stream(mut recv: quinn::RecvStream, len: Byte) -> Result<()> {
    let mut recv_len = 0usize;
    let mut buf = [0u8; 10_000];

    // record the time
    let start = Instant::now();

    while let Some(len) = recv.read(&mut buf).await? {
        recv_len += len;
    }
    let duration = start.elapsed();
    let bytes_per_sec = (recv_len as f64) / duration.as_secs_f64();

    if recv_len > 0 {
        eprintln!(
            "received {} data in {:?} - {}/s",
            len.get_adjusted_unit(ByteUnit::MB),
            duration,
            Byte::from(bytes_per_sec as u64).get_appropriate_unit(true)
        );
    }

    Ok(())
}

async fn handle_send_stream(mut send: quinn::SendStream, len: Byte) -> Result<()> {
    let up_len = len.get_bytes() as u64;
    let mut chunks = vec![Bytes::new(); 16];
    let mut data = testing::Data::new(up_len);

    // record the time
    let start = Instant::now();

    while let Some(count) = data.send(usize::MAX, &mut chunks) {
        for chunk in chunks.iter_mut().take(count) {
            // `take` drops chunk at the end of the loop and replace it with empty Bytes
            let w_chunk = core::mem::take(chunk);
            send.write_all(&w_chunk).await?;
        }
    }
    send.finish().await?;

    let duration = start.elapsed();
    let bytes_per_sec = (up_len as f64) / duration.as_secs_f64();

    if up_len > 0 {
        eprintln!(
            "sent {} data in {:?} - {}/s",
            len.get_adjusted_unit(ByteUnit::MB),
            duration,
            Byte::from(bytes_per_sec as u64).get_appropriate_unit(true)
        );
    }

    Ok(())
}
