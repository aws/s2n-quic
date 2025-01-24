mod async_io;
// mod salty_lib;
mod tcp;

pub trait Rpc {
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
        type Response: Response;

        fn create_request<Payload>(
            &self,
            addr: SocketAddr,
            payload: Payload,
        ) -> impl Future<Output = io::Result<Self::Response>>
        where
            Payload: reader::storage::Infallible;
    }

    pub trait Response: Send + Sized {
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
        #[inline]
        fn read_to_end<Payload>(
            mut self,
            payload: &mut Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: writer::Storage,
        {
            async move {
                let mut total = 0;
                loop {
                    let len = self.read_chunk(payload).await?;

                    if len > 0 {
                        total += len;
                        continue;
                    }

                    // we don't need to check if we ran out of space in the payload
                    if payload.has_remaining_capacity() {
                        return Ok(total);
                    }

                    let mut buffer = &mut [0u8][..];
                    let len = self.read_chunk(&mut buffer).await?;
                    // if the stream is actually finished with additional capacity provided, then return the total
                    if len == 0 {
                        return Ok(total);
                    }

                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "the provided payload buffer ran out of space for reading the response",
                    ));
                }
            }
        }
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
        ) -> impl Future<Output = io::Result<(Payload, Self::Response, SocketAddr)>>
        where
            Payload: writer::Storage;
    }

    pub trait Response: Send + Sized {
        /// Writes a single chunk at a time
        ///
        /// Used for streaming responses
        fn write_chunk<Payload>(
            &mut self,
            payload: &mut Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: reader::storage::Infallible;

        /// Writes the entire response from the `Payload`
        #[inline]
        fn write_to_end<Payload>(
            mut self,
            mut payload: Payload,
        ) -> impl Future<Output = io::Result<usize>>
        where
            Payload: reader::storage::Infallible,
        {
            async move {
                let mut total = 0;

                loop {
                    total += self.write_chunk(&mut payload).await?;

                    if payload.buffer_is_empty() {
                        break;
                    }
                }

                Ok(total)
            }
        }
    }
}
