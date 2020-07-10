#![forbid(unsafe_code)]

compliance::specification!(source = "specs/rfc2616.txt#4.2", level = MUST,);

compliance::citation!("specs/rfc2616.txt#4.3",);

compliance::exception!(
    /// If a message is received with both a
    /// Transfer-Encoding header field and a Content-Length header field,
    /// the latter MUST be ignored.
    //
    // # Reason
    // We decided not to do this
    "specs/rfc2616.txt#4.4",
);

#[compliance::implements("specs/rfc2616.txt#4.2")]
pub fn free_fn() -> usize {
    let a = 1;
    let b = 2;
    let c = 3;
    a + b + c
}

#[compliance::implements("specs/rfc2616.txt#1.1")]
#[derive(Debug)]
pub struct Foo {}

#[compliance::implements("specs/rfc2616.txt#1.1")]
impl Foo {
    #[compliance::implements("specs/rfc2616.txt#1.1")]
    pub const FOO: u16 = 123;

    #[compliance::implements("specs/rfc2616.txt#1.1")]
    pub fn foo(&self) {}
}

#[compliance::implements("specs/rfc2616.txt#1.1")]
pub mod testing {
    #[compliance::implements("specs/rfc2616.txt#1.1")]
    pub fn foo() {}
}

#[compliance::implements(
    /// Unrecognized header
    /// fields SHOULD be ignored by the recipient and MUST be forwarded by
    /// transparent proxies.
    "specs/rfc2616.txt#7.1"
)]
fn entity_header_fields() {
    // implementation goes here here
}

#[compliance::tests("specs/rfc2616.txt#7.1")]
#[test]
fn entity_header_fields_test() {
    entity_header_fields();
}

// TODO implement direct urls
// #[compliance::implements(
//     /// Leading zeros MUST be ignored by recipients and
//     /// MUST NOT be sent.
//     "https://tools.ietf.org/rfc/rfc2616.txt#3.1"
// )]
// pub fn doc_fn() {}

#[compliance::implements(
    /// Leading zeros MUST be ignored by recipients and
    /// MUST NOT be sent.
    "specs/rfc2616.txt#3.1"
)]
pub fn local_doc_fn() {}
