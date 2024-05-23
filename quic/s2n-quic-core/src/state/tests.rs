// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use insta::{assert_debug_snapshot, assert_snapshot};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Lr {
    #[default]
    Init,
    Left,
    Right,
    LeftLeft,
    LeftRight,
    RightLeft,
    RightRight,
}

impl Lr {
    event! {
        on_left(
            Init => Left,
            Left => LeftLeft,
            Right => RightLeft,
        );
        on_right(
            Init => Right,
            Left => LeftRight,
            Right => RightRight,
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn lr_snapshots() {
    assert_debug_snapshot!(Lr::test_transitions());
}

#[test]
#[cfg_attr(miri, ignore)]
fn lr_dot_test() {
    assert_snapshot!(Lr::dot());
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Microwave {
    #[default]
    Idle,
    OpenIdle,
    SettingTime,
    OpenSettingTime,
    Paused,
    OpenPaused,
    Running,
}

impl Microwave {
    event! {
        on_number(
            Idle | SettingTime => SettingTime,
            OpenSettingTime => OpenSettingTime,
        );
        on_cancel(
            Idle | SettingTime | Paused | Running => Idle,
            OpenIdle | OpenSettingTime | OpenPaused => OpenIdle,
        );
        on_start(
            SettingTime | Paused | Running => Running,
        );
        on_door_open(
            Idle => OpenIdle,
            SettingTime => OpenSettingTime,
            Paused | Running => OpenPaused,
        );
        on_door_close(
            OpenIdle => Idle,
            OpenSettingTime => SettingTime,
            OpenPaused => Paused,
        );
        on_time_finished(
            Running => Idle,
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn microwave_snapshots() {
    assert_debug_snapshot!(Microwave::test_transitions());
}

#[test]
#[cfg_attr(miri, ignore)]
fn microwave_dot_test() {
    assert_snapshot!(Microwave::dot());
}
