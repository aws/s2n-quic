// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::parser::File;
use std::collections::HashSet;

pub fn validate(files: &[File]) {
    let mut endpoint_names = HashSet::new();
    let mut connection_names = HashSet::new();
    let mut errored = false;

    for file in files {
        for (subject, name) in file
            .structs
            .iter()
            .map(|v| (&v.attrs.subject, v.attrs.event_name.as_ref()))
        {
            let Some(name) = name else {
                continue;
            };

            let set = if subject.is_connection() {
                &mut connection_names
            } else {
                &mut endpoint_names
            };

            if !set.insert(name.value()) {
                let path = file.path.canonicalize().unwrap();
                let path = path.strip_prefix(std::env::current_dir().unwrap()).unwrap();
                eprintln!(
                    "[{}]: Duplicate event name: {:?}, subject = {subject:?}",
                    path.display(),
                    name.value()
                );
                errored = true;
            }
        }
    }

    if errored {
        panic!("Validation errors");
    }
}
