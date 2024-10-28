// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::panic;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Location {
    location: &'static panic::Location<'static>,
    name: String,
}

impl Location {
    #[track_caller]
    pub fn new<N: core::fmt::Display>(name: N) -> Self {
        let location = panic::Location::caller();
        let name = name.to_string();
        Self { location, name }
    }

    #[track_caller]
    #[allow(clippy::manual_map)] // using `Option::map` messes up the track_caller
    pub fn from_thread_name() -> Option<Self> {
        let thread = std::thread::current();

        // only create a location if insta can figure out the test name from the
        // thread
        if let Some(name) = thread.name().filter(|name| *name != "main") {
            let name = name
                .split("::")
                .chain(Some("events"))
                .collect::<Vec<_>>()
                .join("__");
            Some(Self::new(name))
        } else {
            None
        }
    }

    pub fn snapshot_log(&self, output: &[String]) {
        // miri doesn't support the syscalls that insta uses
        if cfg!(miri) {
            return;
        }

        let value = output.join("\n");

        let name = self.name.as_str();

        let mut settings = insta::Settings::clone_current();

        // we want to use the actual caller's module
        settings.set_prepend_module_to_snapshot(false);
        settings.set_input_file(self.location.file());
        settings.set_snapshot_path(self.snapshot_path());
        settings.set_omit_expression(true);

        settings.bind(|| {
            insta::assert_snapshot!(name, &value);
        });
    }

    fn snapshot_path(&self) -> PathBuf {
        let ws = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let file = Path::new(self.location.file());

        let file = if file.is_relative() {
            ws.join(file)
        } else {
            file.to_path_buf()
        };

        file.canonicalize()
            .unwrap()
            .parent()
            .unwrap()
            .join("snapshots")
    }
}
