// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//= https://www.rfc-editor.org/rfc/rfc9000#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

pub use token::Token;

pub mod token;
