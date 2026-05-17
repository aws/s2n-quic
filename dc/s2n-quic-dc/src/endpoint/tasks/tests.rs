// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract-test style guide for `endpoint::tasks`.
//!
//! These tests should model real task interactions, not isolated helpers:
//! use channels between tasks, spawn at least two interacting tasks (driver + task under test),
//! and prefer reacting on channel events over sleeping. Include both positive and negative
//! assertions in each scenario (what must happen and what must not happen). Keep harnesses
//! builder-driven so each test stays focused on behavior rather than setup boilerplate.

mod ack_burst;
mod ack_completion;
mod completion;
mod context_resolver;
mod frame_dispatch;
mod helpers;
mod idle_wheel;
mod invalidation;
mod socket_recv;
mod waker_drain;
