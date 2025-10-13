// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("byte_array_event")]
#[c_type(s2n_byte_array_event)]
struct ByteArrayEvent<'a> {
    data: &'a [u8],
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct s2n_byte_array_event {
    data: *const u8,
    len: u32,
}

impl<'a> IntoEvent<builder::ByteArrayEvent<'a>> for &c_ffi::s2n_byte_array_event {
    fn into_event(self) -> builder::ByteArrayEvent<'a> {
        let data = unsafe {
            std::slice::from_raw_parts(self.data, self.len.try_into().unwrap())
        };
        builder::ByteArrayEvent { data }
    }
}

#[builder_derive(derive(PartialEq))]
enum TestEnum {
    TestValue1,
    TestValue2,
}

#[event("enum_event")]
#[c_type(s2n_enum_event)]
struct EnumEvent {
    value: TestEnum,
}

#[repr(C)]
#[allow(non_camel_case_types)]
enum s2n_test_enum {
    S2N_TEST_VALUE_1,
    S2N_TEST_VALUE_2,
}

impl IntoEvent<builder::TestEnum> for c_ffi::s2n_test_enum {
    fn into_event(self) -> builder::TestEnum {
        match self {
            Self::S2N_TEST_VALUE_1 => builder::TestEnum::TestValue1,
            Self::S2N_TEST_VALUE_2 => builder::TestEnum::TestValue2,
        }
    }
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct s2n_enum_event {
    value: s2n_test_enum,
}

impl IntoEvent<builder::EnumEvent> for &c_ffi::s2n_enum_event {
    fn into_event(self) -> builder::EnumEvent {
        let value = self.value.clone();
        builder::EnumEvent {
            value: value.into_event(),
        }
    }
}
