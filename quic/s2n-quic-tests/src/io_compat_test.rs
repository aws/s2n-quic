// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::prev;
use s2n_quic::provider::io::testing::{test, Model};
use s2n_quic_core::stream::testing::Data;

#[test]
fn handle_layout_compatibility() {
    use s2n_quic::provider::io::testing::Handle as CurrentHandle;
    use s2n_quic_prev::provider::io::testing::Handle as PrevHandle;
    assert_eq!(
        std::mem::size_of::<CurrentHandle>(),
        std::mem::size_of::<PrevHandle>()
    );
    assert_eq!(
        std::mem::align_of::<CurrentHandle>(),
        std::mem::align_of::<PrevHandle>()
    );
    assert_eq!(
        std::any::type_name::<s2n_quic::provider::io::testing::executor::Handle>(),
        std::any::type_name::<s2n_quic_prev::provider::io::testing::executor::Handle>(),
    );
}

#[test]
fn client_ahead_test() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let addr = prev::prev_server(handle, model.clone())?;
        crate::client(handle, addr, model.clone(), true)?;
        Ok(addr)
    })
    .unwrap();
}

#[test]
fn server_ahead_test() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let addr = crate::server(handle, model.clone())?;
        prev::prev_client(handle, addr, model.clone(), true)?;
        Ok(addr)
    })
    .unwrap();
}

#[test]
fn same_version_test() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let addr = crate::server(handle, model.clone())?;
        crate::client(handle, addr, model.clone(), true)?;
        Ok(addr)
    })
    .unwrap();
}

#[test]
fn client_ahead_large_transfer_test() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = prev::prev_build_server(handle, model.clone())?;
        let addr = prev::prev_start_server(server)?;
        let client = crate::build_client(handle, model.clone(), true)?;
        crate::start_client(client, addr, Data::new(100_000))?;
        Ok(addr)
    })
    .unwrap();
}

#[test]
fn server_ahead_large_transfer_test() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = crate::build_server(handle, model.clone())?;
        let addr = crate::start_server(server)?;
        let client = prev::prev_build_client(handle, model.clone(), true)?;
        prev::prev_start_client(
            client,
            addr,
            s2n_quic_core_prev::stream::testing::Data::new(100_000),
        )?;
        Ok(addr)
    })
    .unwrap();
}
