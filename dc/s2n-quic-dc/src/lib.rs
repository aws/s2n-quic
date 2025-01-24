// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod allocator;
pub mod clock;
pub mod congestion;
pub mod control;
pub mod credentials;
pub mod crypto;
pub mod datagram;
pub mod event;
pub mod msg;
pub mod packet;
pub mod path;
pub mod pool;
pub mod random;
pub mod recovery;
pub mod rpc;
pub mod socket;
pub mod stream;
pub mod sync;
pub mod task;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use s2n_quic_core::dc::{Version, SUPPORTED_VERSIONS};

pub trait Transport {
    type Client: client::Client;
    type Server: server::Server;

    fn create_client(&self) -> Self::Client;
    fn create_server(&self, listen_addr: std::net::SocketAddr) -> Self::Server;
}

pub mod client {
    use core::future::Future;
    use s2n_quic_core::buffer::{reader, writer};
    use std::{io, net::SocketAddr};

    pub trait Client {
        type Response<Payload>: Response
        where
            Payload: reader::Storage;

        fn create_request<Payload>(
            &self,
            addr: SocketAddr,
            payload: Payload,
        ) -> impl Future<Output = io::Result<Self::Response<Payload>>>
        where
            Payload: reader::Storage;
    }

    pub trait Response: Send {
        /// Reads a single chunk at a time
        ///
        /// Used for streaming responses and operating on them incrementally
        fn read_chunk<Payload>(
            &mut self,
            payload: &mut Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: writer::Storage;

        /// Reads the entire response to the `Payload`
        fn read_to_end<Payload>(
            self,
            payload: &mut Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: writer::Storage;
    }
}

pub mod server {
    use core::future::Future;
    use s2n_quic_core::buffer::{reader, writer};
    use std::{io, net::SocketAddr};

    pub trait Server: 'static + Send + Sync {
        type Response: Response;

        fn accept_request<Payload>(
            &self,
            payload: Payload,
        ) -> impl Future<Output = io::Result<(SocketAddr, Payload, Self::Response)>>
        where
            Payload: writer::Storage;
    }

    pub trait Response: Send {
        /// Writes a single chunk at a time
        ///
        /// Used for streaming responses
        fn write_chunk<Payload>(
            &mut self,
            payload: &mut Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: reader::Storage;

        /// Writes the entire response from the `Payload`
        fn write_to_end<Payload>(self, payload: Payload) -> impl Future<Output = io::Result<usize>>
        where
            Payload: reader::Storage;
    }
}

#[cfg(test)]
mod examples {
    use super::{
        client::{self, Response as _},
        server::{self, Response as _},
    };
    use bytes::{Buf, Bytes};
    use s2n_quic_core::buffer::reader;
    use std::{io, net::SocketAddr};

    async fn client_request_response(
        client: &impl client::Client,
        addr: SocketAddr,
    ) -> io::Result<()> {
        let response = client.create_request(addr, &b"hello world!"[..]).await?;

        let mut out: Vec<u8> = vec![];
        response.read_to_end(&mut out).await?;

        Ok(())
    }

    async fn client_request_response_copy_avoidance(
        client: &impl client::Client,
        addr: SocketAddr,
    ) -> io::Result<()> {
        // chain some chunks together to simulate coming from different places in memory
        let mut request = b"hello ".chain(&b"world!"[..]);
        let request = reader::storage::Buf::new(&mut request);

        let response = client.create_request(addr, request).await?;

        // instead of copying into a single contiguous region of memory, append it to a list of chunks
        let mut out: Vec<Bytes> = vec![];
        response.read_to_end(&mut out).await?;

        Ok(())
    }

    /// You would spawn this N times, where N is the maximum request concurrency you want to allow
    ///
    /// This should work a bit better than the more traditional `accept` + `spawn`, since it actually
    /// applies backpressure to the acceptor which can start to reject connections as the server
    /// is under load.
    async fn server_request_response(
        server: &impl server::Server,
        mut payload: impl reader::Storage,
    ) -> io::Result<()> {
        let mut request: Vec<Bytes> = vec![];
        loop {
            let (addr, req, response) = server.accept_request(request).await?;

            // do something with the request - in this example we just put the buffer back
            request = req;

            response.write_to_end(&b"hello!"[..]).await?;
        }
    }
}
