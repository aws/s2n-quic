// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
#[path = "./overload-server/mod.rs"]
mod overload_server;

fn main() {
    #[cfg(not(unix))]
    eprintln!("not supported yet outside cfg(unix)");

    #[cfg(unix)]
    overload_server::main().unwrap();
}
