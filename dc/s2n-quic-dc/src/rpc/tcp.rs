use crate::rpc::{self, async_io};

mod client {
    use super::*;
    use rpc::server::Response as _;
    use s2n_quic_core::buffer::reader;
    use std::{future::Future, io, net::SocketAddr};
    use tokio::net::TcpStream;

    #[derive(Clone, Default)]
    pub struct Client {
        // TODO config
    }

    impl Client {
        #[inline]
        async fn connect(&self, addr: SocketAddr) -> io::Result<TcpStream> {
            // TODO apply config if applicable
            TcpStream::connect(addr).await
        }
    }

    impl rpc::client::Client for Client {
        type Response = async_io::Reader<TcpStream>;

        #[inline]
        fn create_request<Payload>(
            &self,
            addr: SocketAddr,
            payload: Payload,
        ) -> impl Future<Output = io::Result<Self::Response>>
        where
            Payload: reader::storage::Infallible,
        {
            async move {
                let mut stream = self.connect(addr).await?;

                async_io::Writer::new(async_io::Borrowed::new(&mut stream))
                    .write_to_end(payload)
                    .await?;

                Ok(async_io::Reader::new(stream))
            }
        }
    }
}

mod server {
    use super::*;
    use rpc::client::Response as _;
    use s2n_quic_core::buffer::writer;
    use std::{future::Future, io, net::SocketAddr};
    use tokio::net::{TcpListener, TcpStream};

    pub struct Server {
        listener: TcpListener,
    }

    impl Server {
        #[inline]
        pub fn new(listener: TcpListener) -> Self {
            Self { listener }
        }
    }

    impl rpc::server::Server for Server {
        type Response = async_io::Writer<TcpStream>;

        #[inline]
        fn accept_request<Payload>(
            &self,
            mut payload: Payload,
        ) -> impl Future<Output = io::Result<(Payload, Self::Response, SocketAddr)>>
        where
            Payload: writer::Storage,
        {
            async {
                let (mut stream, addr) = self.listener.accept().await?;

                async_io::Reader::new(async_io::Borrowed::new(&mut stream))
                    .read_to_end(&mut payload)
                    .await?;

                Ok((payload, async_io::Writer::new(stream), addr))
            }
        }
    }
}
