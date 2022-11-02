// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{TransportParameter, TransportParameterId, TransportParameterValidator};
use core::{fmt, marker::PhantomData};

/// Struct for marking a field as disabled for a given endpoint type
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct DisabledParameter<T>(PhantomData<T>);

impl<T> fmt::Debug for DisabledParameter<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DisabledParameter")
    }
}

impl<T> Default for DisabledParameter<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: TransportParameter> TransportParameter for DisabledParameter<T> {
    type CodecValue = ();

    const ENABLED: bool = false;
    const ID: TransportParameterId = T::ID;

    fn from_codec_value(_value: Self::CodecValue) -> Self {
        Self(Default::default())
    }

    fn try_into_codec_value(&self) -> Option<&Self::CodecValue> {
        None
    }

    fn default_value() -> Self {
        Self(Default::default())
    }
}

impl<T> TransportParameterValidator for DisabledParameter<T> {}
