mod error_code;
mod keyset;

pub use error_code::*;
pub use keyset::KeySet;

/// Extension trait for errors that have an associated [`ApplicationErrorCode`]
pub trait ApplicationErrorExt {
    /// Returns the associated [`ApplicationErrorCode`], if any
    fn application_error_code(&self) -> Option<ApplicationErrorCode>;
}
