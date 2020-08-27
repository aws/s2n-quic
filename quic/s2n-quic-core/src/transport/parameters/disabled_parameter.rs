use super::{TransportParameter, TransportParameterID, TransportParameterValidator};
use core::marker::PhantomData;

/// Struct for marking a field as disabled for a given endpoint type
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DisabledParameter<T>(PhantomData<T>);

impl<T> Default for DisabledParameter<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: TransportParameter> TransportParameter for DisabledParameter<T> {
    type CodecValue = ();

    const ENABLED: bool = false;
    const ID: TransportParameterID = T::ID;

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
