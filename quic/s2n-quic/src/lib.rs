// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![doc = r#"
An implementation of the IETF QUIC protocol

### Server Example

```rust,no_run
use std::{error::Error, path::Path};
use s2n_quic::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut server = Server::builder()
        .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
        .with_io("127.0.0.1:443")?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            eprintln!("Connection accepted from {:?}", connection.remote_addr());

            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
                    eprintln!("Stream opened from {:?}", stream.connection().remote_addr());

                    // echo any data back to the stream
                    while let Ok(Some(data)) = stream.receive().await {
                        stream.send(data).await.expect("stream should be open");
                    }
                });
            }
        });
    }

    Ok(())
}
```
"#]

#[macro_use]
pub mod provider;

pub mod connection;
pub mod server;
pub mod stream;

pub mod application {
    pub use s2n_quic_core::application::Error;
}

pub use connection::Connection;
pub use server::Server;

#[cfg(feature = "protocol-extensions")]
mod extensions;
#[cfg(feature = "protocol-extensions")]
pub use extensions::Extensions;
