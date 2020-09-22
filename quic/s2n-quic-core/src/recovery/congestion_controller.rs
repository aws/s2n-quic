pub trait CongestionController: Clone {
    // TODO implement callbacks
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct MockCC {
        // TODO add fields
        _todo: (),
    }

    impl CongestionController for MockCC {
        // TODO implement callbacks
    }
}
