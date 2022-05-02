// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bolero::generator::*;
use core::time::Duration;
use s2n_quic_core::{ack, inet::ExplicitCongestionNotification};

pub fn gen_ack_settings() -> impl ValueGenerator<Output = ack::Settings> {
    (gen_duration(), 0..20).map(|(max_ack_delay, ack_delay_exponent)| ack::Settings {
        max_ack_delay,
        ack_delay_exponent,
        ..Default::default()
    })
}

pub fn gen_default<V: Default>() -> impl ValueGenerator<Output = V> {
    gen::<()>().map(|()| V::default())
}

pub fn gen_duration() -> impl ValueGenerator<Output = Duration> {
    ValueGenerator::map(1u16..10_000, |millis| Duration::from_millis(millis as u64))
}

pub fn gen_ecn() -> impl ValueGenerator<Output = ExplicitCongestionNotification> {
    gen::<bool>().map(|has_congestion| {
        if has_congestion {
            ExplicitCongestionNotification::Ce
        } else {
            ExplicitCongestionNotification::default()
        }
    })
}
