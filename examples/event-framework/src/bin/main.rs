// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use event_framework::{print_event, query_event};
use s2n_quic::Server;
use std::error::Error;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
/// NOTE: this certificate is to be used for demonstration purposes only!
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // It's possible to compose different Subscribers, each of which is responsible
    // for a different task.
    //
    // Note: subscriber is implemented for `(A, B)` and therefore requires the nested tuple
    let composed_event_subscriber = (
        // Our custom query subscriber allows us to store information on a custom
        // connection context and query it from the application.
        //
        // There are two types of queries:
        // - connection.query_event_context: yields a reference to the connection context
        // which can be used read data from the connection context.
        // - connection.query_event_context_mut: yields a mutable reference to the connection
        // context which can be used to read and write data to the connection context.
        query_event::MyQuerySubscriber,
        (
            // Our custom print subscriber allows us to print events to stdout.
            print_event::MyPrintSubscriber {
                print_all_events: true,
                print_connection_events: false,
            },
            // The tracing subscriber will allow applications to configure and use
            // [tracing](https://docs.rs/tracing/latest/tracing/) for instrumentation.
            s2n_quic::provider::event::tracing::Subscriber::default(),
        ),
    );
    // Build an `s2n_quic::Server`
    let mut server = Server::builder()
        .with_event(composed_event_subscriber)?
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io("127.0.0.1:4433")?
        .start()?;

    let mut connection_count = 0;
    while let Some(mut connection) = server.accept().await {
        // Change packet count behavior after processing some number of connections.
        //
        // Note: The context type, `&query_event::MyQueryContext`, provided in the query must
        // match the context on one of the subscribers. See the docs on `query_event_context_mut`
        // for a detailed explanation on how a query is executed on composed subscribers.
        //
        // The application can mutably access the connection context and modify data on
        // the context itself.
        if connection_count > 5 {
            connection.query_event_context_mut(|context: &mut query_event::MyQueryContext| {
                context.count_non_data_packets = false;
            })?;
        } else {
            connection_count += 1;
        }

        // Query the packet sent count and print it.
        //
        // Note: The context type, `&query_event::MyQueryContext`, provided in the query must
        // match the context on one of the subscribers. See the docs on `query_event_context`
        // for a detailed explanation on how a query is executed on composed subscribers.
        //
        // The application can immutably access the connection context and read data from it.
        connection.query_event_context(|context: &query_event::MyQueryContext| {
            println!(
                "{:?} data packets have been processed",
                context.packet_sent_count
            )
        })?;

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
