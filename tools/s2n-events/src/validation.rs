// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::parser::File;
use std::collections::HashSet;

pub fn validate(files: &[File]) {
    let mut endpoint_names = HashSet::new();
    let mut connection_names = HashSet::new();
    let mut errored = false;

    for file in files {
        let path = file.path.canonicalize().unwrap();
        let path = path
            .strip_prefix(std::env::current_dir().unwrap())
            .unwrap()
            .to_owned();

        for s in &file.structs {
            if let Some(name) = s.attrs.event_name.as_ref() {
                let set = if s.attrs.subject.is_connection() {
                    &mut connection_names
                } else {
                    &mut endpoint_names
                };

                if !set.insert(name.value()) {
                    eprintln!(
                        "[{}]: Duplicate event name: {:?}, subject = {:?}",
                        path.display(),
                        name.value(),
                        s.attrs.subject,
                    );
                    errored = true;
                }
            }

            for field in &s.fields {
                if !is_float_type(&field.ty) {
                    continue;
                }

                let field_name = field
                    .ident
                    .as_ref()
                    .map(|i| i.to_string())
                    .unwrap_or_default();

                for metrics in [
                    &field.attrs.counter,
                    &field.attrs.measure_counter,
                    &field.attrs.nominal_counter,
                    &field.attrs.measure,
                    &field.attrs.gauge,
                ] {
                    for metric in metrics {
                        let valid = metric
                            .unit
                            .as_ref()
                            .map(|u| u == "Percent" || u == "Float")
                            .unwrap_or(false);

                        if !valid {
                            eprintln!(
                                "[{}]: field `{field_name}` is f32/f64 but metric {:?} \
                                 does not specify `Percent` or `Float` unit",
                                path.display(),
                                metric.name.value(),
                            );
                            errored = true;
                        }
                    }
                }
            }
        }
    }

    if errored {
        panic!("Validation errors");
    }
}

fn is_float_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            let name = seg.ident.to_string();
            return name == "f32" || name == "f64";
        }
    }
    false
}
