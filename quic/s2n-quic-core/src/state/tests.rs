// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use insta::{assert_debug_snapshot, assert_snapshot};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Init,
    Left,
    Right,
    LeftLeft,
    LeftRight,
    RightLeft,
    RightRight,
}

impl State {
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
fn snapshots() {
    let mut outcomes = vec![];
    let states = [
        State::Init,
        State::Left,
        State::Right,
        State::LeftLeft,
        State::LeftRight,
        State::RightLeft,
        State::RightRight,
    ];
    for state in states {
        macro_rules! push {
            ($event:ident) => {
                let mut target = state.clone();
                let result = target.$event().map(|_| target);
                outcomes.push((state.clone(), stringify!($event), result));
            };
        }
        push!(on_left);
        push!(on_right);
    }

    assert_debug_snapshot!(outcomes);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dot_test() {
    assert_snapshot!(State::dot());
}
