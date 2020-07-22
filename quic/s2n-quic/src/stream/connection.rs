macro_rules! impl_connection_api {
    (| $self:ident | $convert:expr) => {
        /// Returns the [`Connection`] associated with the [`Stream`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn connection(&self) -> $crate::connection::Connection {
            let $self = self;
            $convert
        }
    };
}
