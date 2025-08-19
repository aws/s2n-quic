// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)] // we just want to make sure these APIs are present

use super::Stream;
use std::{io, net::SocketAddr};
use tokio::io::{AsyncRead, AsyncWrite};

/// Shows that the stream type can get the underlying peer_addr/local_addr
fn addr_test(stream: Stream) {
    let _: io::Result<SocketAddr> = stream.peer_addr();
    let _: io::Result<SocketAddr> = stream.local_addr();

    let (read, write) = stream.into_split();

    let _: io::Result<SocketAddr> = read.peer_addr();
    let _: io::Result<SocketAddr> = read.local_addr();

    let _: io::Result<SocketAddr> = write.peer_addr();
    let _: io::Result<SocketAddr> = write.local_addr();
}

fn assert_debug<T: core::fmt::Debug>(_v: &T) {}
fn assert_send<T: Send>(_v: &T) {}
fn assert_sync<T: Sync>(_v: &T) {}
fn assert_read<T: AsyncRead>(_v: &T) {}
fn assert_write<T: AsyncWrite>(_v: &T) {}

/// Shows that the stream type implements all of the expected traits
fn traits_test(stream: Stream) {
    assert_debug(&stream);
    assert_send(&stream);
    assert_sync(&stream);
    assert_read(&stream);
    assert_write(&stream);

    let (read, write) = stream.into_split();

    assert_debug(&read);
    assert_send(&read);
    assert_sync(&read);
    assert_read(&read);

    assert_debug(&write);
    assert_send(&write);
    assert_sync(&write);
    assert_write(&write);
}
