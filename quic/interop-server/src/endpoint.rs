use crate::socket::Socket;
use bytes::Bytes;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::{select, FutureExt};
use s2n_quic_core::{
    connection::ConnectionId, endpoint::EndpointType, io::tx::Tx as TxTrait, stream::StreamType,
    transport::parameters,
};
use s2n_quic_platform::time;
use s2n_quic_rustls::{rustls, RustlsServerEndpoint, RustlsServerSession};
use s2n_quic_transport::{
    acceptor::Acceptor,
    connection::{Connection, ConnectionConfig, ConnectionImpl, ConnectionLimits},
    endpoint::{ConnectionIdGenerator, Endpoint as QuicEndpoint, EndpointConfig},
    stream::{Stream, StreamError, StreamImpl},
};
use std::io;

type QuicEndpointType = QuicEndpoint<InteropEndpointConfig>;

/// A primitive connection ID generator for testing purposes.
struct LocalConnectionIdGenerator {
    next_id: u64,
}

impl LocalConnectionIdGenerator {
    fn new() -> Self {
        Self { next_id: 1 }
    }
}

impl ConnectionIdGenerator for LocalConnectionIdGenerator {
    type DestinationConnectionIDDecoderType = usize;

    fn generate_connection_id(&mut self) -> (ConnectionId, Option<Duration>) {
        let id = self.next_id;
        self.next_id += 1;
        let bytes = id.to_be_bytes();

        let id = ConnectionId::try_from_bytes(&bytes[..]).unwrap();
        (id, None)
    }

    fn destination_connection_id_decoder(&self) -> Self::DestinationConnectionIDDecoderType {
        8
    }
}

/// Connection configurations we use for the interop server
pub struct InteropConnectionConfig {
    // TODO: Add the full transport parameters
    local_flow_control_limits: parameters::InitialFlowControlLimits,
    local_ack_settings: parameters::AckSettings,
    connection_id_decoder: usize,
    connection_limits: ConnectionLimits,
}

impl ConnectionConfig for InteropConnectionConfig {
    type DestinationConnectionIDDecoderType = usize;
    type StreamType = StreamImpl;
    type TLSSession = RustlsServerSession;

    const ENDPOINT_TYPE: EndpointType = EndpointType::Server;

    fn local_flow_control_limits(&self) -> &parameters::InitialFlowControlLimits {
        &self.local_flow_control_limits
    }

    fn local_ack_settings(&self) -> &parameters::AckSettings {
        &self.local_ack_settings
    }

    fn destination_connnection_id_decoder(&self) -> Self::DestinationConnectionIDDecoderType {
        self.connection_id_decoder
    }

    fn connection_limits(&self) -> &ConnectionLimits {
        &self.connection_limits
    }
}

struct InteropEndpointConfig {
    // TODO: Add the full transport parameters
    local_flow_control_limits: parameters::InitialFlowControlLimits,
    local_ack_settings: parameters::AckSettings,
}

impl EndpointConfig for InteropEndpointConfig {
    type ConnectionConfigType = InteropConnectionConfig;
    type ConnectionIdGeneratorType = LocalConnectionIdGenerator;
    type ConnectionType = ConnectionImpl<Self::ConnectionConfigType>;
    type TLSEndpointType = RustlsServerEndpoint;

    fn create_connection_config(&mut self) -> Self::ConnectionConfigType {
        InteropConnectionConfig {
            local_flow_control_limits: self.local_flow_control_limits,
            local_ack_settings: self.local_ack_settings,
            connection_id_decoder: 8,
            connection_limits: Default::default(),
        }
    }
}

pub struct Endpoint {
    endpoint: QuicEndpointType,
    acceptor: Acceptor,
    socket: Socket,
}

impl Endpoint {
    pub fn new(
        socket: Socket,
        tls_config: rustls::ServerConfig,
        transport_parameters: parameters::ServerTransportParameters,
    ) -> Self {
        let endpoint_config = InteropEndpointConfig {
            local_flow_control_limits: transport_parameters.flow_control_limits(),
            local_ack_settings: transport_parameters.ack_settings(),
        };

        let tls_endpoint = RustlsServerEndpoint::new(tls_config, transport_parameters);

        let connection_id_generator = LocalConnectionIdGenerator::new();
        let (endpoint, acceptor) =
            QuicEndpointType::new(endpoint_config, connection_id_generator, tls_endpoint);

        Self {
            acceptor,
            endpoint,
            socket,
        }
    }

    pub fn listen(self) -> (impl Future<Output = Result<(), io::Error>>, Acceptor) {
        async fn listen(
            mut socket: Socket,
            mut endpoint: QuicEndpointType,
        ) -> Result<(), io::Error> {
            loop {
                let max_sleep_time = match endpoint.next_timer_expiration() {
                    Some(timeout) => timeout.saturating_duration_since(time::now()),
                    None => Duration::from_millis(1000),
                };

                select! {
                    receive_result = socket.sync_rx().fuse() => {
                        // We received packets on the UDP socket
                        let received_packets = receive_result?;
                        if received_packets > 0 {
                            endpoint.receive(&mut socket.rx, time::now());
                        }
                    }
                    _ = tokio::time::delay_for(max_sleep_time).fuse() => {
                        // Do nothing. We will check timers in each iteration anyway
                    }
                    _nr_wakeups = WaitForWakeupFuture { endpoint: &mut endpoint }.fuse() => {
                        // Do nothing. Checking for wakeups is performed in the Future
                    }
                }

                endpoint.handle_timers(time::now());

                endpoint.transmit(&mut socket.tx, time::now());

                // TODO: This will wait until all messages had been transmitted, but
                // we should receive and handle user calls in parallel.
                while !socket.tx.is_empty() {
                    socket.sync_tx().await?;
                }
            }
        }

        let listener = listen(self.socket, self.endpoint);
        let acceptor = self.acceptor;

        (listener, acceptor)
    }
}

struct WaitForWakeupFuture<'a> {
    endpoint: &'a mut QuicEndpointType,
}

impl<'a> Future for WaitForWakeupFuture<'a> {
    // Returns the number of wakeups
    type Output = usize;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // In every poll iteration we check whether a wakeup occurred and try to handle it
        // If a wakeup occurred we return `Ready` and resolve the `Future`. Otherwise we continue
        // to wait for wakeups (as well as other events like timeouts).
        self.endpoint.poll_pending_wakeups(cx, time::now())
    }
}

pub trait AcceptExt {
    fn accept(&mut self) -> AcceptFuture;
}

impl AcceptExt for Acceptor {
    fn accept(&mut self) -> AcceptFuture {
        AcceptFuture { acceptor: self }
    }
}

pub struct AcceptFuture<'a> {
    acceptor: &'a mut Acceptor,
}

impl<'a> Future for AcceptFuture<'a> {
    // Returns a Connection
    type Output = Connection;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.acceptor.poll_accept(cx)
    }
}

pub trait ConnectionExt {
    fn accept(&mut self, stream_type: StreamType) -> StreamAcceptFuture;
}

impl ConnectionExt for Connection {
    fn accept(&mut self, stream_type: StreamType) -> StreamAcceptFuture {
        StreamAcceptFuture {
            connection: self,
            stream_type,
        }
    }
}

pub struct StreamAcceptFuture<'a> {
    connection: &'a mut Connection,
    stream_type: StreamType,
}

impl<'a> Future for StreamAcceptFuture<'a> {
    // Returns a Stream
    type Output = Result<Stream, StreamError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let stream_type = self.stream_type;
        self.connection.poll_accept(stream_type, cx)
    }
}

pub trait StreamExt {
    fn pop(&mut self) -> StreamPopFuture;
    fn push(&mut self, data: Bytes) -> StreamPushFuture;
    fn finish(&mut self) -> StreamFinishFuture;
}

impl StreamExt for Stream {
    fn pop(&mut self) -> StreamPopFuture {
        StreamPopFuture { stream: self }
    }

    fn push(&mut self, data: Bytes) -> StreamPushFuture {
        StreamPushFuture { stream: self, data }
    }

    fn finish(&mut self) -> StreamFinishFuture {
        StreamFinishFuture { stream: self }
    }
}

pub struct StreamPopFuture<'a> {
    stream: &'a mut Stream,
}

impl<'a> Future for StreamPopFuture<'a> {
    type Output = Result<Option<Bytes>, StreamError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.stream.poll_pop(cx)
    }
}

pub struct StreamPushFuture<'a> {
    stream: &'a mut Stream,
    data: Bytes,
}

impl<'a> Future for StreamPushFuture<'a> {
    type Output = Result<(), StreamError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let data = self.data.clone();
        self.stream.poll_push(data, cx)
    }
}

pub struct StreamFinishFuture<'a> {
    stream: &'a mut Stream,
}

impl<'a> Future for StreamFinishFuture<'a> {
    type Output = Result<(), StreamError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.stream.poll_finish(cx)
    }
}
