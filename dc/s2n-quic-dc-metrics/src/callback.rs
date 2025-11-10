// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{any::Any, fmt::Write};

use crate::Unit;

/// This is a helper trait that is used for the `Registry::register_list_callback` method, which
/// allows registering callbacks for a particular metric to be called when needed.
pub(crate) trait ValueList {
    /// This is called to display the current value of this metric.
    fn take_current(&mut self) -> Option<String>;

    /// Returns any for the self type, used to cast back when registering equally-named metrics.
    fn as_any(&mut self) -> &mut dyn Any;
}

impl<D, F> ValueList for (Vec<F>, Unit)
where
    F: FnMut() -> D + Send + 'static,
    D: std::fmt::Display,
{
    fn take_current(&mut self) -> Option<String> {
        let mut output = String::new();
        let mut first = true;
        for callback in self.0.iter_mut() {
            if !first {
                output.push('+');
            }
            first = false;
            write!(output, "{}", (callback)()).unwrap();
        }

        output.push_str(self.1.pmet_str());
        Some(output)
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
