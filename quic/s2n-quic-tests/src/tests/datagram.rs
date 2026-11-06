// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::datagram::{
    default::{Endpoint as DefaultEndpoint, Receiver as DefaultReceiver, Sender as DefaultSender},
    ConnectionInfo, Endpoint, Packet, PreConnectionInfo, ReceiveContext, Receiver, Sender,
};
use s2n_quic_core::connection;

/// Records the value of `datagrams_prioritized` for each packet
#[derive(Clone, Default)]
struct RecordingEndpoint {
    prioritized: Arc<Mutex<Vec<bool>>>,
}

struct RecordingSender {
    prioritized: Arc<Mutex<Vec<bool>>>,
}

struct NoopReceiver;

impl Endpoint for RecordingEndpoint {
    type Sender = RecordingSender;
    type Receiver = NoopReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        let sender = RecordingSender {
            prioritized: self.prioritized.clone(),
        };
        (sender, NoopReceiver)
    }

    fn max_datagram_frame_size(&self, _info: &PreConnectionInfo) -> u64 {
        0
    }
}

impl Sender for RecordingSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        self.prioritized
            .lock()
            .unwrap()
            .push(packet.datagrams_prioritized());
    }

    fn has_transmission_interest(&self) -> bool {
        false
    }

    fn on_connection_error(&mut self, _error: connection::Error) {}
}

impl Receiver for NoopReceiver {
    fn on_datagram(&mut self, _context: &ReceiveContext<'_>, _datagram: &[u8]) {}

    fn on_connection_error(&mut self, _error: connection::Error) {}
}

/// Ensures datagrams are prioritized in every other packet of a connection
#[test]
fn datagrams_prioritized_alternates_between_packets() {
    let model = Model::default();
    let endpoint = RecordingEndpoint::default();
    let prioritized = endpoint.prioritized.clone();

    test(model.clone(), |handle| {
        let server_addr = server(handle, model.clone())?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(123))?
            .with_datagram(endpoint)?
            .start()?;

        start_client(client, server_addr, Data::new(100_000))
    })
    .unwrap();

    let prioritized = prioritized.lock().unwrap();
    assert!(prioritized.len() > 1);
    assert!(
        prioritized.windows(2).any(|pair| pair[0] != pair[1]),
        "datagrams_prioritized never changed across {} packets",
        prioritized.len()
    );
}

/// Ensures a queued datagram doesn't wait for all of the pending stream data to be sent
#[test]
fn datagram_is_sent_while_stream_data_is_pending() {
    const CHUNK_LEN: usize = 10_000;
    const CHUNKS: usize = 100;
    const STREAM_LEN: usize = CHUNK_LEN * CHUNKS;

    let model = Model::default();
    let received_before_datagram = Arc::new(Mutex::new(None));
    let result = received_before_datagram.clone();

    test(model.clone(), |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(123))?
            .with_datagram(DefaultEndpoint::builder().build().unwrap())?
            .start()?;
        let server_addr = server.local_addr()?;

        spawn(async move {
            let mut connection = server.accept().await.unwrap();
            let mut stream = connection.accept_receive_stream().await.unwrap().unwrap();

            let mut received = 0;
            while let Some(chunk) = stream.receive().await.unwrap() {
                received += chunk.len();

                let datagram = connection
                    .datagram_mut(|receiver: &mut DefaultReceiver| receiver.recv_datagram())
                    .unwrap();

                let mut result = result.lock().unwrap();
                if datagram.is_some() && result.is_none() {
                    *result = Some(received);
                }
            }
            assert_eq!(received, STREAM_LEN);
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(123))?
            .with_datagram(DefaultEndpoint::builder().build().unwrap())?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();
            let mut stream = connection.open_send_stream().await.unwrap();

            let chunk = Bytes::from(vec![42; CHUNK_LEN]);
            stream.send(chunk.clone()).await.unwrap();

            connection
                .datagram_mut(|sender: &mut DefaultSender| {
                    sender.send_datagram(Bytes::from_static(b"datagram"))
                })
                .unwrap()
                .unwrap();

            for _ in 1..CHUNKS {
                stream.send(chunk.clone()).await.unwrap();
            }
            stream.finish().unwrap();

            // keep the connection open until the server has read the whole stream
            delay(Duration::from_secs(5)).await;
        });

        Ok(())
    })
    .unwrap();

    let received = received_before_datagram
        .lock()
        .unwrap()
        .unwrap_or(STREAM_LEN);
    assert!(
        received < STREAM_LEN,
        "datagram was only delivered after the whole stream"
    );
}
