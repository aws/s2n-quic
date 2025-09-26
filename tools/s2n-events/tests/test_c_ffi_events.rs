// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod c_ffi_events;

pub use c_ffi_events::event;
use c_ffi_events::event::ConnectionPublisher;
use s2n_quic_core::{
    event::IntoEvent,
    time::{testing::Clock as MockClock, Clock},
};
use std::time::Duration;

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

#[test]
fn convert_byte_array_event() {
    let data: Vec<u8> = vec![4, 5, 6];
    let len = data.len();
    let event = &event::c_ffi::s2n_byte_array_event {
        data: data.as_ptr(),
        len: len as u32,
    };

    let converted = event.into_event();

    assert_eq!(converted.data, data.as_slice());
}

#[test]
fn convert_enum_event() {
    struct TestCase {
        value: event::c_ffi::s2n_test_enum,
        expected_value: event::builder::TestEnum,
    }

    let test_cases = [
        TestCase {
            value: event::c_ffi::s2n_test_enum::S2N_TEST_VALUE_1,
            expected_value: event::builder::TestEnum::TestValue1,
        },
        TestCase {
            value: event::c_ffi::s2n_test_enum::S2N_TEST_VALUE_2,
            expected_value: event::builder::TestEnum::TestValue2,
        },
    ];

    for test in test_cases {
        let event = &event::c_ffi::s2n_enum_event { value: test.value };

        let converted = event.into_event();

        assert_eq!(converted.value, test.expected_value);
    }
}

#[test]
fn convert_connection_meta() {
    let meta = &event::c_ffi::s2n_event_connection_meta {
        timestamp: Duration::new(1, 0).as_nanos() as u64,
    };

    let converted = meta.into_event();

    assert_eq!(
        converted.timestamp.duration_since_start(),
        Duration::new(1, 0)
    );
}
