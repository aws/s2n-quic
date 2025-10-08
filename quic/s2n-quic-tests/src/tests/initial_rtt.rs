// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::{
    inet::SocketAddress,
    recovery::{DEFAULT_INITIAL_RTT, MIN_RTT},
};

/// This test demonstrates that setting initial RTT to smaller values than the default
/// results in more PTO packets being sent based on the smaller initial value.
#[test]
fn set_initial_rtt() {
    let pto_count = test_with_initial_rtt(DEFAULT_INITIAL_RTT);
    assert_eq!(3, pto_count);

    let pto_count = test_with_initial_rtt(Duration::from_millis(1));
    assert_eq!(11, pto_count);

    let pto_count = test_with_initial_rtt(MIN_RTT);
    assert_eq!(13, pto_count);
}

#[should_panic]
#[test]
fn invalid_initial_rtt() {
    test_with_initial_rtt(MIN_RTT - Duration::from_nanos(1));
}

fn test_with_initial_rtt(initial_rtt: Duration) -> usize {
    let model = Model::default();
    let pto_subscriber = recorder::Pto::new();
    let pto_events = pto_subscriber.events();

    test(model.clone(), |handle| {
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((
                tracing_events(true, model.max_udp_payload()),
                pto_subscriber,
            ))?
            .with_random(Random::with_seed(456))?
            .with_limits(
                provider::limits::Limits::default()
                    .with_initial_round_trip_time(initial_rtt)
                    .unwrap(),
            )?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(SocketAddress::default()).with_server_name("localhost");
            // We would expect this connection to time out since there is no server started
            assert!(client.connect(connect).await.is_err());
        });

        Ok(SocketAddress::default())
    })
    .unwrap();

    let pto_events = pto_events.lock().unwrap();
    let pto_count = *pto_events.iter().max().unwrap_or(&0) as usize;

    pto_count
}
