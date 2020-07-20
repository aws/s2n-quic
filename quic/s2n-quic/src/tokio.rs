use crate::provider::runtime;

#[derive(Debug, Default)]
pub struct Runtime {
    // TODO
}

impl runtime::Provider for Runtime {}

#[tokio::test]
async fn tokio_test() {
    // stub test to satisfy udeps until we actually use tokio
}
