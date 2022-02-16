// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Example of a print subscriber which can print all events or only
/// connection related events.
pub mod print_event {
    use s2n_quic::provider::{event, event::ConnectionMeta};

    #[derive(Debug, Clone)]
    pub struct MyPrintSubscriber {
        // prints all events, including connection events
        pub print_all_events: bool,
        // prints only connection related events
        pub print_connection_events: bool,
    }

    impl event::Subscriber for MyPrintSubscriber {
        type ConnectionContext = ();

        fn create_connection_context(
            &mut self,
            meta: &ConnectionMeta,
            info: &event::ConnectionInfo,
        ) -> Self::ConnectionContext {
            println!("{:?} {:?}", meta, info);
        }

        fn on_event<M: event::Meta + core::fmt::Debug, E: event::Event + core::fmt::Debug>(
            &mut self,
            meta: &M,
            event: &E,
        ) {
            if self.print_all_events {
                println!("event: {:?} {:?}", meta, event);
            }
        }

        fn on_connection_event<E: event::Event + core::fmt::Debug>(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &E,
        ) {
            if self.print_connection_events {
                println!("connection_event: {:?} {:?} {:?}", context, meta, event);
            }
        }
    }
}

/// Example of a query subscriber which can be used to store event information; which
/// can then be queried from the application.
pub mod query_event {
    use std::time::Duration;

    use s2n_quic::provider::{
        event,
        event::{events, ConnectionMeta},
    };

    #[derive(Debug, Clone, Default, Copy)]
    pub struct MyQueryContext {
        // Record how many non application packets are received
        pub non_data_packet_count: usize,
        // Record how many application data packets are received
        pub data_packet_count: usize,
        // The ratio of packets used to initialize a connection vs packets used
        // for transmitting data.
        pub overhead_ratio: f64,
        // Last time the overhead was updated
        pub overhead_updated: Duration,
    }

    impl MyQueryContext {
        // Reset the packet count and ratio.
        pub fn reset(&mut self) {
            *self = MyQueryContext {
                overhead_updated: self.overhead_updated,
                ..Default::default()
            };
        }
    }

    #[derive(Default)]
    pub struct MyQuerySubscriber;

    impl event::Subscriber for MyQuerySubscriber {
        type ConnectionContext = MyQueryContext;

        /// Initialize the Connection Context that is passed to the `supervisor_timeout` and
        /// `on_supervisor_timeout` methods, as well as each connection-related event.
        fn create_connection_context(
            &mut self,
            _meta: &events::ConnectionMeta,
            _info: &events::ConnectionInfo,
        ) -> Self::ConnectionContext {
            MyQueryContext::default()
        }

        fn on_packet_sent(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &events::PacketSent,
        ) {
            match event.packet_header {
                events::PacketHeader::ZeroRtt { .. } | events::PacketHeader::OneRtt { .. } => {
                    context.data_packet_count += 1;
                    // we update the ratio here to avoid dividing by 0
                    context.overhead_updated = meta.timestamp.duration_since_start();
                    context.overhead_ratio =
                        context.non_data_packet_count as f64 / context.data_packet_count as f64;
                }
                _ => context.non_data_packet_count += 1,
            }
        }

        fn on_packet_received(
            &mut self,
            context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            event: &events::PacketReceived,
        ) {
            match event.packet_header {
                events::PacketHeader::ZeroRtt { .. } | events::PacketHeader::OneRtt { .. } => {
                    context.data_packet_count += 1;
                    // we update the ratio here to avoid dividing by 0
                    context.overhead_updated = meta.timestamp.duration_since_start();
                    context.overhead_ratio =
                        context.non_data_packet_count as f64 / context.data_packet_count as f64;
                }
                _ => context.non_data_packet_count += 1,
            }
        }
    }
}
