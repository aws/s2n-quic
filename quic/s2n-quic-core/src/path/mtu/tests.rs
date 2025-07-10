// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{testing::*, *};
use crate::{
    endpoint,
    event::testing::Publisher,
    frame::Frame,
    inet::{IpV4Address, SocketAddressV4},
    packet::number::PacketNumberSpace,
    path::{mtu, mtu::Manager},
    recovery::congestion_controller::testing::mock::CongestionController,
    time::{clock::testing::now, timer::Provider as _},
    transmission::{
        writer::{
            testing::{OutgoingFrameBuffer, Writer as MockWriteContext},
            Writer as _,
        },
        Provider as _,
    },
    varint::VarInt,
};
use std::{convert::TryInto, net::SocketAddr};

/// Creates an application space packet number with the given value
fn pn(nr: usize) -> PacketNumber {
    PacketNumberSpace::ApplicationData.new_packet_number(VarInt::new(nr as u64).unwrap())
}

#[test]
fn mtu_config_is_valid() {
    let config = Config {
        initial_mtu: 1500.try_into().unwrap(),
        base_mtu: 1228.try_into().unwrap(),
        max_mtu: 9000.try_into().unwrap(),
    };

    assert!(config.is_valid());

    let config = Config {
        initial_mtu: 1500.try_into().unwrap(),
        base_mtu: 1500.try_into().unwrap(),
        max_mtu: 1500.try_into().unwrap(),
    };

    assert!(config.is_valid());

    let config = Config {
        initial_mtu: 1500.try_into().unwrap(),
        base_mtu: 1501.try_into().unwrap(),
        max_mtu: 9000.try_into().unwrap(),
    };

    assert!(!config.is_valid());

    let config = mtu::Config {
        initial_mtu: 1500.try_into().unwrap(),
        base_mtu: 1228.try_into().unwrap(),
        max_mtu: 1400.try_into().unwrap(),
    };

    assert!(!config.is_valid());
}

#[test]
fn mtu_config_builder() {
    // Default built config is valid
    assert!(mtu::Config::builder().build().unwrap().is_valid());

    // Setting the base MTU higher than the default adjusts the default initial MTU
    let builder = mtu::Config::builder();
    let builder = builder.with_base_mtu(1300).unwrap();
    let config = builder.build().unwrap();
    assert_eq!(1300_u16, u16::from(config.base_mtu));
    assert_eq!(1300_u16, u16::from(config.initial_mtu));

    // Setting the base MTU higher than the default max MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder
        .with_base_mtu(DEFAULT_MAX_MTU.0.get() + 1_u16)
        .unwrap()
        .build();
    assert_eq!(Some(MtuError), result.err());

    // Setting the initial MTU higher than the default max MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder
        .with_initial_mtu(DEFAULT_MAX_MTU.0.get() + 1_u16)
        .unwrap()
        .build();
    assert_eq!(Some(MtuError), result.err());

    // Setting the base MTU higher than the configured initial MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_initial_mtu(1300).unwrap().with_base_mtu(1301);
    assert_eq!(Some(MtuError), result.err());

    // Setting the max MTU lower than the configured initial MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_initial_mtu(1300).unwrap().with_max_mtu(1299);
    assert_eq!(Some(MtuError), result.err());

    // Setting the initial MTU lower than the configured base MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_base_mtu(1300).unwrap().with_initial_mtu(1299);
    assert_eq!(Some(MtuError), result.err());

    // Setting the initial MTU higher than the configured max MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_max_mtu(1300).unwrap().with_initial_mtu(1301);
    assert_eq!(Some(MtuError), result.err());

    // Setting the base MTU higher than the configured max MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_max_mtu(1300).unwrap().with_base_mtu(1301);
    assert_eq!(Some(MtuError), result.err());

    // Setting the max MTU lower than the configured base MTU results in an error
    let builder = mtu::Config::builder();
    let result = builder.with_base_mtu(1300).unwrap().with_max_mtu(1299);
    assert_eq!(Some(MtuError), result.err());
}

#[test]
fn mtu_manager() {
    let remote = inet::SocketAddress::default();
    let endpoint_config = mtu::Config::builder().build().unwrap();
    assert!(endpoint_config.is_valid());

    // valid: with Default config
    let mtu_provider = mtu::Config::builder().build().unwrap();
    assert!(mtu_provider.is_valid());
    let mut manager: Manager<Config> = Manager::new(mtu_provider);
    manager.config(&remote).unwrap();

    // valid: max_mtu == endpoint_config.max_mtu
    let mtu_provider = mtu::Config::builder()
        .with_max_mtu(DEFAULT_MAX_MTU.into())
        .unwrap()
        .build()
        .unwrap();
    assert!(mtu_provider.is_valid());
    let mut manager: Manager<Config> = Manager::new(mtu_provider);
    manager.config(&remote).unwrap();

    // invalid: !mtu_provider.is_valid()
    let mtu_provider = mtu::Config {
        initial_mtu: InitialMtu::MIN,
        base_mtu: BaseMtu(NonZeroU16::new(1500).unwrap()),
        max_mtu: MaxMtu::MIN,
    };
    assert!(!mtu_provider.is_valid());
    let mut manager: Manager<Config> = Manager::new(mtu_provider);
    assert_eq!(manager.config(&remote).unwrap_err(), MtuError);

    // invalid: mtu_provider.max_mtu > endpoint_config.max_mtu
    let mtu_provider = mtu::Config::builder()
        .with_max_mtu(1501)
        .unwrap()
        .build()
        .unwrap();
    assert!(mtu_provider.is_valid());
    let mut manager: Manager<Config> = Manager::new(mtu_provider);
    assert_eq!(manager.config(&remote).unwrap_err(), MtuError);
}

#[test]
fn base_plpmtu_is_1200() {
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
    //= type=test
    //# When using
    //# IPv4, there is no currently equivalent size specified, and a
    //# default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
    let ip = IpV4Address::new([127, 0, 0, 1]);
    let addr = inet::SocketAddress::IpV4(SocketAddressV4::new(ip, 443));
    let controller = Controller::new(Config::default(), &addr);
    assert_eq!(controller.base_plpmtu, 1200);
}

#[test]
fn min_max_mtu() {
    // Use an IPv6 address to force a smaller `max_udp_payload`
    let addr: SocketAddr = "[::1]:123".parse().unwrap();
    let controller = Controller::new(
        Config {
            max_mtu: MaxMtu::MIN,
            ..Default::default()
        },
        &addr.into(),
    );
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE, controller.plpmtu);
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE, controller.base_plpmtu);
    assert!(controller.is_search_completed());
}

#[test]
fn new_max_mtu_smaller_than_common_mtu() {
    let max_mtu = MINIMUM_MAX_DATAGRAM_SIZE + UDP_HEADER_LEN + IPV4_MIN_HEADER_LEN + 1;

    let mut controller = new_controller(max_mtu);
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE + 1, controller.probed_size);
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE, controller.base_plpmtu);

    controller.enable();
    assert_eq!(State::SearchComplete, controller.state);
}

#[test]
fn new_ipv4() {
    let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
    let controller = Controller::new(
        Config {
            max_mtu: 1600.try_into().unwrap(),
            ..Default::default()
        },
        &addr.into(),
    );
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE, controller.base_plpmtu);
    assert_eq!(
        1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.max_udp_payload
    );
    assert_eq!(
        1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.max_probe_size
    );
    assert_eq!(
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        controller.max_datagram_size()
    );
    assert_eq!(0, controller.probe_count);
    assert_eq!(State::Disabled, controller.state);
    assert!(!controller.pmtu_raise_timer.is_armed());
    assert_eq!(
        ETHERNET_MTU - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.probed_size
    );
}

#[test]
fn new_ipv6() {
    let addr: SocketAddr = "[2001:0db8:85a3:0001:0002:8a2e:0370:7334]:9000"
        .parse()
        .unwrap();
    let controller = Controller::new(
        Config {
            max_mtu: 2000.try_into().unwrap(),
            ..Default::default()
        },
        &addr.into(),
    );
    assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE, controller.base_plpmtu);
    assert_eq!(
        2000 - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
        controller.max_udp_payload
    );
    assert_eq!(
        2000 - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
        controller.max_probe_size
    );
    assert_eq!(
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        controller.max_datagram_size()
    );
    assert_eq!(0, controller.probe_count);
    assert_eq!(State::Disabled, controller.state);
    assert!(!controller.pmtu_raise_timer.is_armed());
    assert_eq!(
        ETHERNET_MTU - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
        controller.probed_size
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
//= type=test
//# Datagrams containing Initial packets MAY exceed 1200 bytes if the sender
//# believes that the network path and peer both support the size that it chooses.
#[test]
fn new_initial_and_base_mtu() {
    let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
    let mut controller = Controller::new(
        Config {
            max_mtu: 2600.try_into().unwrap(),
            base_mtu: 1400.try_into().unwrap(),
            initial_mtu: 2500.try_into().unwrap(),
        },
        &addr.into(),
    );
    assert_eq!(
        2600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.max_udp_payload
    );
    assert_eq!(
        2600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.max_probe_size
    );
    assert_eq!(
        1400 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.base_plpmtu
    );
    assert_eq!(
        2500 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.plpmtu
    );
    assert_eq!(
        (2500 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN) as usize,
        controller.max_datagram_size()
    );
    assert_eq!(0, controller.probe_count);
    assert_eq!(State::EarlySearchRequested, controller.state);
    assert!(!controller.pmtu_raise_timer.is_armed());
    // probe a value halfway to the max mtu
    assert_eq!(
        2550 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.probed_size
    );
    controller.enable();
    assert!(matches!(controller.state, State::SearchRequested));
}

#[test]
fn new_initial_mtu_less_than_ethernet_mtu() {
    let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
    let mut controller = Controller::new(
        Config {
            max_mtu: 9000.try_into().unwrap(),
            initial_mtu: 1400.try_into().unwrap(),
            ..Default::default()
        },
        &addr.into(),
    );
    // probe the ethernet MTU
    assert_eq!(
        ETHERNET_MTU - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.probed_size
    );
    controller.enable();
    assert!(matches!(controller.state, State::SearchRequested));
}

#[test]
fn new_initial_mtu_equal_to_ethernet_mtu() {
    let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
    let mut controller = Controller::new(
        Config {
            max_mtu: 9000.try_into().unwrap(),
            initial_mtu: ETHERNET_MTU.try_into().unwrap(),
            ..Default::default()
        },
        &addr.into(),
    );
    // probe halfway to the max MTU
    assert_eq!(
        1500 + (9000 - 1500) / 2 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
        controller.probed_size
    );
    controller.enable();
    assert!(matches!(controller.state, State::SearchRequested));
}

#[test]
fn enable_already_enabled() {
    let mut controller = new_controller(1500);
    assert_eq!(State::Disabled, controller.state);
    controller.enable();
    assert_eq!(State::SearchRequested, controller.state);
    controller.state = State::SearchComplete;
    controller.enable();
    assert_eq!(State::SearchComplete, controller.state);
}

#[test]
fn enable() {
    let mut controller = new_controller(1500);
    assert_eq!(State::Disabled, controller.state);
    controller.enable();
    assert_eq!(State::SearchRequested, controller.state);
}

//= https://www.rfc-editor.org/rfc/rfc8899#section-4.2
//= type=test
//# When
//# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
//# reception of a probe packet.
#[test]
fn on_packet_ack_within_threshold() {
    let mut controller = new_controller(1472 + PROBE_THRESHOLD * 2);
    let max_udp_payload = controller.max_udp_payload;
    let pn = pn(1);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.state = State::Searching(pn, now);
    controller.probed_size = MINIMUM_MAX_DATAGRAM_SIZE;
    controller.max_probe_size = MINIMUM_MAX_DATAGRAM_SIZE + PROBE_THRESHOLD * 2 - 1;

    let result = controller.on_packet_ack(
        pn,
        MINIMUM_MAX_DATAGRAM_SIZE,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );
    assert_eq!(MtuResult::MtuUpdated(MINIMUM_MAX_DATAGRAM_SIZE), result);

    assert_eq!(
        MINIMUM_MAX_DATAGRAM_SIZE + (max_udp_payload - MINIMUM_MAX_DATAGRAM_SIZE) / 2,
        controller.probed_size
    );
    assert_eq!(1, cc.on_mtu_update);
    assert_eq!(State::SearchComplete, controller.state);
    assert!(controller.pmtu_raise_timer.is_armed());
    assert_eq!(
        Some(now + PMTU_RAISE_TIMER_DURATION),
        controller.next_expiration()
    );
    assert!(controller.is_search_completed());

    // Enough time passes that its time to try raising the PMTU again
    let now = now + PMTU_RAISE_TIMER_DURATION;
    controller.on_timeout(now);

    assert_eq!(State::SearchRequested, controller.state);
    assert_eq!(
        MINIMUM_MAX_DATAGRAM_SIZE + (max_udp_payload - MINIMUM_MAX_DATAGRAM_SIZE) / 2,
        controller.probed_size
    );
    assert!(!controller.is_search_completed());
}

#[test]
fn on_packet_ack_within_threshold_of_max_plpmtu() {
    let mut controller = new_controller(1472 + (PROBE_THRESHOLD * 2 - 1));
    let max_udp_payload = controller.max_udp_payload;
    let pn = pn(1);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.state = State::Searching(pn, now);

    let probed_sized = controller.probed_size;
    let result = controller.on_packet_ack(
        pn,
        probed_sized,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );

    assert_eq!(MtuResult::MtuUpdated(probed_sized), result);
    assert_eq!(1472 + (max_udp_payload - 1472) / 2, controller.probed_size);
    assert_eq!(1, cc.on_mtu_update);
    assert_eq!(State::SearchComplete, controller.state);
    assert!(!controller.pmtu_raise_timer.is_armed());
    assert!(controller.is_search_completed());
}

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.3.2
//= type=test
//# Implementations SHOULD select the set of probe packet sizes to
//# maximize the gain in PLPMTU from each search step.

//= https://www.rfc-editor.org/rfc/rfc8899#section-8
//= type=test
//# To avoid excessive load, the interval between individual probe
//# packets MUST be at least one RTT, and the interval between rounds of
//# probing is determined by the PMTU_RAISE_TIMER.
#[test]
fn on_packet_ack_search_requested() {
    let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
    let max_udp_payload = controller.max_udp_payload;
    let pn = pn(1);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.state = State::Searching(pn, now);

    let probed_size = controller.probed_size;
    let result = controller.on_packet_ack(
        pn,
        probed_size,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );

    assert_eq!(MtuResult::MtuUpdated(probed_size), result);
    assert_eq!(1472 + (max_udp_payload - 1472) / 2, controller.probed_size);
    assert_eq!(1, cc.on_mtu_update);
    assert_eq!(State::SearchRequested, controller.state);
    assert!(!controller.pmtu_raise_timer.is_armed());
    assert!(!controller.is_search_completed());
}

#[test]
fn on_packet_ack_resets_black_hole_counter() {
    let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
    let pnum = pn(3);
    let mut cc = CongestionController::default();
    let mut publisher = Publisher::snapshot();
    controller.enable();

    controller.black_hole_counter += 1;
    // ack a packet smaller than the plpmtu
    let result = controller.on_packet_ack(
        pnum,
        controller.plpmtu - 1,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );
    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(controller.black_hole_counter, 1);
    assert_eq!(None, controller.largest_acked_mtu_sized_packet);

    // ack a packet the size of the plpmtu
    let result = controller.on_packet_ack(
        pnum,
        controller.plpmtu,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );
    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(controller.black_hole_counter, 0);
    assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);

    controller.black_hole_counter += 1;

    // ack an older packet
    let pnum_2 = pn(2);
    let result = controller.on_packet_ack(
        pnum_2,
        controller.plpmtu,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );
    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(controller.black_hole_counter, 1);
    assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
}

#[test]
fn on_packet_ack_disabled_controller() {
    let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
    let pnum = pn(3);
    let mut cc = CongestionController::default();
    let mut publisher = Publisher::snapshot();

    controller.black_hole_counter += 1;
    controller.largest_acked_mtu_sized_packet = Some(pnum);

    let pn = pn(10);
    let result = controller.on_packet_ack(
        pn,
        controller.plpmtu,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );

    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(State::Disabled, controller.state);
    assert_eq!(controller.black_hole_counter, 1);
    assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
}

#[test]
fn on_packet_ack_not_application_space() {
    let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
    let pnum = pn(3);
    let mut cc = CongestionController::default();
    let mut publisher = Publisher::snapshot();
    controller.enable();

    controller.black_hole_counter += 1;
    controller.largest_acked_mtu_sized_packet = Some(pnum);

    // on_packet_ack will be called with packet numbers from Initial and Handshake space,
    // so it should not fail in this scenario.
    let pn = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));
    let result = controller.on_packet_ack(
        pn,
        controller.plpmtu,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );
    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(controller.black_hole_counter, 1);
    assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
}

//= https://www.rfc-editor.org/rfc/rfc8899#section-3
//= type=test
//# The PL is REQUIRED to be
//# robust in the case where probe packets are lost due to other
//# reasons (including link transmission error, congestion).
#[test]
fn on_packet_loss() {
    let mut controller = new_controller(1500);
    let max_udp_payload = controller.max_udp_payload;
    let pn = pn(1);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.state = State::Searching(pn, now);
    let probed_size = controller.probed_size;

    let result = controller.on_packet_loss(
        pn,
        controller.probed_size,
        false,
        now,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );

    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(0, cc.on_mtu_update);
    assert_eq!(max_udp_payload, controller.max_probe_size);
    assert_eq!(probed_size, controller.probed_size);
    assert_eq!(State::SearchRequested, controller.state);
}

#[test]
fn on_packet_loss_max_probes() {
    let mut controller = new_controller(1500);
    let max_udp_payload = controller.max_udp_payload;
    let pn = pn(1);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.state = State::Searching(pn, now);
    controller.probe_count = MAX_PROBES;
    assert_eq!(max_udp_payload, controller.max_probe_size);

    let result = controller.on_packet_loss(
        pn,
        controller.probed_size,
        false,
        now,
        &mut cc,
        path::Id::test_id(),
        &mut publisher,
    );

    assert_eq!(MtuResult::NoChange, result);
    assert_eq!(0, cc.on_mtu_update);
    assert_eq!(1472, controller.max_probe_size);
    assert_eq!(
        MINIMUM_MAX_DATAGRAM_SIZE + (1472 - MINIMUM_MAX_DATAGRAM_SIZE) / 2,
        controller.probed_size
    );
    assert_eq!(State::SearchRequested, controller.state);
}

#[test]
fn on_packet_loss_black_hole() {
    let mut controller = new_controller(1500);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    controller.plpmtu = 1472;
    controller.enable();
    let base_plpmtu = controller.base_plpmtu;

    for i in 0..BLACK_HOLE_THRESHOLD + 1 {
        let pn = pn(i as usize);

        // Losing a packet the size of the BASE_PLPMTU should not increase the black_hole_counter
        let result = controller.on_packet_loss(
            pn,
            base_plpmtu,
            true,
            now,
            &mut cc,
            path::Id::test_id(),
            &mut publisher,
        );
        assert_eq!(MtuResult::NoChange, result);
        assert_eq!(controller.black_hole_counter, i);

        // Losing a packet larger than the PLPMTU should not increase the black_hole_counter
        let result = controller.on_packet_loss(
            pn,
            controller.plpmtu + 1,
            true,
            now,
            &mut cc,
            path::Id::test_id(),
            &mut publisher,
        );
        assert_eq!(MtuResult::NoChange, result);
        assert_eq!(controller.black_hole_counter, i);

        // Losing a packet that does not start a new loss burst should not increase the black_hole_counter
        let result = controller.on_packet_loss(
            pn,
            base_plpmtu + 1,
            false,
            now,
            &mut cc,
            path::Id::test_id(),
            &mut publisher,
        );
        assert_eq!(MtuResult::NoChange, result);
        assert_eq!(controller.black_hole_counter, i);

        let result = controller.on_packet_loss(
            pn,
            base_plpmtu + 1,
            true,
            now,
            &mut cc,
            path::Id::test_id(),
            &mut publisher,
        );
        if i < BLACK_HOLE_THRESHOLD {
            assert_eq!(MtuResult::NoChange, result);
            assert_eq!(controller.black_hole_counter, i + 1);
        } else {
            assert_eq!(MtuResult::MtuUpdated(MINIMUM_MAX_DATAGRAM_SIZE), result);
        }
    }

    assert_eq!(controller.black_hole_counter, 0);
    assert_eq!(None, controller.largest_acked_mtu_sized_packet);
    assert_eq!(1, cc.on_mtu_update);
    assert_eq!(base_plpmtu, controller.plpmtu);
    assert_eq!(State::SearchComplete, controller.state);
    assert_eq!(
        Some(now + BLACK_HOLE_COOL_OFF_DURATION),
        controller.pmtu_raise_timer.next_expiration()
    );
    assert!(controller.is_search_completed());
}

#[test]
fn on_packet_loss_disabled_controller() {
    let mut controller = new_controller(1500);
    let mut cc = CongestionController::default();
    let now = now();
    let mut publisher = Publisher::snapshot();
    let base_plpmtu = controller.base_plpmtu;

    for i in 0..BLACK_HOLE_THRESHOLD + 1 {
        let pn = pn(i as usize);
        assert_eq!(controller.black_hole_counter, 0);
        let result = controller.on_packet_loss(
            pn,
            base_plpmtu + 1,
            false,
            now,
            &mut cc,
            path::Id::test_id(),
            &mut publisher,
        );
        assert_eq!(MtuResult::NoChange, result);
    }

    assert_eq!(State::Disabled, controller.state);
    assert_eq!(controller.black_hole_counter, 0);
    assert_eq!(0, cc.on_mtu_update);
}

#[test]
fn on_packet_loss_not_application_space() {
    let mut controller = new_controller(1500);
    let mut cc = CongestionController::default();
    let mut publisher = Publisher::snapshot();
    let base_plpmtu = controller.base_plpmtu;

    // test the loss in each state
    for state in [
        State::Disabled,
        State::SearchRequested,
        State::Searching(pn(1), now()),
        State::SearchComplete,
    ] {
        controller.state = state;
        for i in 0..BLACK_HOLE_THRESHOLD + 1 {
            // on_packet_loss may be called with packet numbers from Initial and Handshake space
            // so it should not fail in this scenario.
            let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(i));
            let result = controller.on_packet_loss(
                pn,
                base_plpmtu + 1,
                false,
                now(),
                &mut cc,
                path::Id::test_id(),
                &mut publisher,
            );
            assert_eq!(MtuResult::NoChange, result);
            assert_eq!(controller.black_hole_counter, 0);
            assert_eq!(0, cc.on_mtu_update);
        }
    }
}

// Tests that when packet loss occurs after initial MTU has been
// configured to a value larger than the default, the MTU drops to
// the base_plpmtu
#[test]
fn on_packet_loss_initial_mtu_configured() {
    let ip = IpV4Address::new([127, 0, 0, 1]);
    let addr = inet::SocketAddress::IpV4(SocketAddressV4::new(ip, 443));
    let mut publisher = Publisher::snapshot();

    for max_mtu in [
        MINIMUM_MTU,
        MINIMUM_MTU + 10,
        1300,
        1450,
        1500,
        1520,
        4000,
        9000,
    ] {
        for initial_mtu in [
            MINIMUM_MTU,
            MINIMUM_MTU + 10,
            1300,
            1450,
            1500,
            1520,
            4000,
            9000,
        ] {
            for base_mtu in [
                MINIMUM_MTU,
                MINIMUM_MTU + 10,
                1300,
                1450,
                1500,
                1520,
                4000,
                9000,
            ] {
                let mtu_config = Config {
                    max_mtu: max_mtu.try_into().unwrap(),
                    initial_mtu: initial_mtu.min(max_mtu).try_into().unwrap(),
                    base_mtu: base_mtu.min(initial_mtu).min(max_mtu).try_into().unwrap(),
                };
                let mut controller = Controller::new(mtu_config, &addr);
                let base_plpmtu = controller.base_plpmtu;
                let original_plpmtu = controller.plpmtu;
                let pn = pn(1);
                let mut cc = CongestionController::default();
                let now = now();

                let result = controller.on_packet_loss(
                    pn,
                    original_plpmtu,
                    false,
                    now,
                    &mut cc,
                    path::Id::test_id(),
                    &mut publisher,
                );

                if original_plpmtu > base_plpmtu {
                    // the MTU was updated
                    assert_eq!(MtuResult::MtuUpdated(base_plpmtu), result);
                    assert_eq!(
                        1, cc.on_mtu_update,
                        "base {base_plpmtu} init {initial_mtu} max {max_mtu} original_plpmtu {original_plpmtu}, base_plpmtu {base_plpmtu}"
                    );
                    assert_eq!(base_plpmtu, controller.plpmtu);
                } else {
                    // everything remains the same since we are operating at the base plpmtu
                    assert_eq!(MtuResult::NoChange, result);
                    assert_eq!(0, cc.on_mtu_update);
                    assert_eq!(original_plpmtu, controller.plpmtu);
                }

                if controller.probed_sized() - controller.max_datagram_size()
                    < PROBE_THRESHOLD as usize
                {
                    assert_eq!(State::SearchComplete, controller.state);
                    assert!(controller.is_search_completed());
                } else {
                    // MTU controller is still disabled
                    assert_eq!(State::Disabled, controller.state);
                }
            }
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
//= type=test
//# When used with an
//# acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
//# generate PLPMTU probes in this state.
#[test]
fn on_transmit_search_not_requested() {
    let mut controller = new_controller(1500);
    controller.state = State::SearchComplete;
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::MtuProbing,
        endpoint::Type::Server,
    );

    controller.on_transmit(&mut write_context);
    assert!(frame_buffer.is_empty());
    assert_eq!(State::SearchComplete, controller.state);
}

#[test]
fn on_transmit_not_mtu_probing() {
    let mut controller = new_controller(1500);
    controller.state = State::SearchRequested;
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );

    controller.on_transmit(&mut write_context);
    assert!(frame_buffer.is_empty());
    assert_eq!(State::SearchRequested, controller.state);

    controller.state = State::SearchComplete;
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );

    controller.on_transmit(&mut write_context);
    assert!(frame_buffer.is_empty());
    assert_eq!(State::SearchComplete, controller.state);
}

#[test]
fn on_transmit_no_capacity() {
    let mut controller = new_controller(1500);
    controller.state = State::SearchRequested;
    let mut frame_buffer = OutgoingFrameBuffer::new();
    frame_buffer.set_max_packet_size(Some(controller.probed_size as usize - 1));
    let mut write_context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::MtuProbing,
        endpoint::Type::Server,
    );

    controller.on_transmit(&mut write_context);
    assert!(frame_buffer.is_empty());
    assert_eq!(State::SearchComplete, controller.state);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
//= type=test
//# Endpoints could limit the content of PMTU probes to PING and PADDING
//# frames, since packets that are larger than the current maximum
//# datagram size are more likely to be dropped by the network.

//= https://www.rfc-editor.org/rfc/rfc8899#section-3
//= type=test
//# Probe loss recovery: It is RECOMMENDED to use probe packets that
//# do not carry any user data that would require retransmission if
//# lost.

//= https://www.rfc-editor.org/rfc/rfc8899#section-4.1
//= type=test
//# DPLPMTUD MAY choose to use only one of these methods to simplify the
//# implementation.
#[test]
fn on_transmit() {
    let mut controller = new_controller(1500);
    controller.state = State::SearchRequested;
    let now = now();
    let mut frame_buffer = OutgoingFrameBuffer::new();
    frame_buffer.set_max_packet_size(Some(controller.probed_size as usize));
    let mut write_context = MockWriteContext::new(
        now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::MtuProbing,
        endpoint::Type::Server,
    );
    let packet_number = write_context.packet_number();

    controller.on_transmit(&mut write_context);
    assert_eq!(0, write_context.remaining_capacity());
    assert_eq!(
        Frame::Ping(frame::Ping),
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );
    assert_eq!(
        Frame::Padding(frame::Padding {
            length: controller.probed_size as usize - 1
        }),
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );
    assert_eq!(State::Searching(packet_number, now), controller.state);
}
