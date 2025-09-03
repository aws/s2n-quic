// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod c_ffi_events;

pub use c_ffi_events::event;
use c_ffi_events::event::ConnectionPublisher;
use s2n_quic_core::{
    event::IntoEvent,
    time::{testing::Clock as MockClock, Clock},
};

#[test]
fn publish_byte_array_event() {
    struct MySubscriber {
        received_data: Vec<u8>,
    }

    impl event::Subscriber for MySubscriber {
        type ConnectionContext = ();

        fn create_connection_context(
            &mut self,
            _meta: &event::api::ConnectionMeta,
            _info: &event::api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }

        fn on_byte_array_event(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &event::api::ConnectionMeta,
            event: &event::api::ByteArrayEvent,
        ) {
            self.received_data.extend_from_slice(event.data);
        }
    }

    let mut subscriber = MySubscriber {
        received_data: Vec::new(),
    };

    let timestamp = MockClock::default().get_time().into_event();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        event::builder::ConnectionMeta { id: 0, timestamp },
        0,
        &mut subscriber,
        &mut context,
    );

    publisher.on_byte_array_event(event::builder::ByteArrayEvent { data: &[1, 2, 3] });

    assert_eq!(subscriber.received_data, vec![1, 2, 3]);
}
