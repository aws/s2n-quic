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

pub use connection::Connection;
pub use s2n_quic_core::application::ApplicationErrorCode;
pub use server::Server;

#[cfg(feature = "protocol-extensions")]
mod extensions;
#[cfg(feature = "protocol-extensions")]
pub use extensions::Extensions;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
//= type=TODO
//= tracking-issue=389
//# A client SHOULD NOT reuse a NEW_TOKEN token for different connection
//# attempts.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
//= type=TODO
//= tracking-issue=395
//# Clients MUST NOT send NEW_TOKEN frames.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
//= type=TODO
//= tracking-issue=390
//# A client MUST NOT include
//# a token that is not applicable to the server that it is connecting
//# to, unless the client has the knowledge that the server that issued
//# the token and the server the client is connecting to are jointly
//# managing the tokens.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
//= type=TODO
//= tracking-issue=390
//# When connecting to a server for
//# which the client retains an applicable and unused token, it SHOULD
//# include that token in the Token field of its Initial packet.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
//= type=TODO
//= tracking-issue=390
//# A client MAY use a token from any previous
//# connection to that server.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
//= type=TODO
//= tracking-issue=392
//# Clients MUST ensure that UDP datagrams containing Initial packets
//# have UDP payloads of at least 1200 bytes, adding PADDING frames as
//# necessary.
