//! This module contains abstractions around the platform on which the
//! stack is running

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

pub mod buffer;
pub mod io;
pub mod message;
pub mod time;
