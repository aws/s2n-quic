// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Acceptor registry for routing requests to registered handlers.
//!
//! This module provides infrastructure for applications to register acceptors that receive
//! requests identified by a VarInt key.
//!
//! # Example
//!
//! ```rust
//! use s2n_quic_core::varint::VarInt;
//! use std::sync::Arc;
//!
//! # use s2n_quic_dc::acceptor::{Acceptor, Dispatch, PendingAction, Registry};
//! # use s2n_quic_dc::flow::queue::AutoWake;
//! struct MyAcceptor;
//!
//! impl Acceptor<String> for MyAcceptor {
//!     fn handle_request(&self, request: String) -> Result<AutoWake, s2n_quic_dc::acceptor::Reject<String>> {
//!         // Spawn a task to handle the request
//!         tokio::spawn(async move {
//!             println!("Handling request: {}", request);
//!         });
//!         Ok(AutoWake::new(None))
//!     }
//!
//!     fn handle_pending(&self, request: String) -> Result<Dispatch, s2n_quic_dc::acceptor::Reject<String>> {
//!         // For pending requests that can't be confirmed as non-duplicate,
//!         // accept them but request a retry to confirm
//!         println!("Handling pending request: {}", request);
//!         Ok(Dispatch {
//!             action: PendingAction::AcceptedWithRetry,
//!             waker: AutoWake::new(None),
//!         })
//!     }
//! }
//!
//! # async fn example() {
//! let registry = Registry::new();
//! let acceptor = Arc::new(MyAcceptor);
//!
//! // Register the acceptor with ID 1
//! let _handle = registry.register(VarInt::from_u8(1), acceptor).unwrap();
//!
//! // Look up and dispatch confirmed request directly
//! registry.with_acceptor(VarInt::from_u8(1), |acceptor| {
//!     acceptor.handle_request("hello".to_string()).unwrap();
//! });
//! # }
//! ```

pub mod channel;

use crate::flow::queue::AutoWake;
use dashmap::DashMap;
use rustc_hash::FxBuildHasher;
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

/// Result of dispatching a pending request to an acceptor.
pub struct Dispatch {
    pub action: PendingAction,
    pub waker: AutoWake,
}

/// Rejection returned by an acceptor.
#[derive(Debug)]
pub struct Reject<T> {
    /// The original request value being rejected.
    ///
    /// Ownership is returned to the caller so endpoint dispatch can perform
    /// cleanup (for example, disabling stream drop side effects) before
    /// emitting reset frames.
    pub request: T,
    /// The endpoint reset reason to send to the peer.
    pub reset: crate::endpoint::error::Error,
}

impl<T> Reject<T> {
    #[inline]
    pub fn new(request: T, reset: crate::endpoint::error::Error) -> Self {
        Self { request, reset }
    }

    #[inline]
    pub fn reset_code(&self) -> VarInt {
        self.reset.as_varint()
    }
}

/// Trait for handling incoming requests
pub trait Acceptor<T>: Send + Sync + 'static {
    /// Called when a new request arrives.
    ///
    /// Returns an `AutoWake` that should be forwarded to the waker thread.
    fn handle_request(&self, request: T) -> Result<AutoWake, Reject<T>>;

    /// Called when a request arrives but cannot be confirmed as non-duplicate.
    ///
    /// Returns a `Dispatch` indicating the action to take and an `AutoWake`.
    fn handle_pending(&self, request: T) -> Result<Dispatch, Reject<T>> {
        Err(Reject::new(
            request,
            crate::endpoint::error::Error::Unknown(VarInt::from_u32(0)),
        ))
    }
}

/// Action to take after attempting to dispatch a pending request
#[derive(Debug)]
pub enum PendingAction {
    /// The acceptor accepted the request and no retry is needed
    Accepted,
    /// The acceptor accepted the request but wants a retry message sent
    /// to confirm it's not a duplicate
    AcceptedWithRetry,
}

/// Handle for keeping an acceptor registered
pub struct Handle {
    acceptor_id: VarInt,
    registry: RegistryInner,
}

impl Handle {
    /// Get the acceptor ID for this handle
    pub fn acceptor_id(&self) -> VarInt {
        self.acceptor_id
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.registry.acceptors.unregister(self.acceptor_id);
    }
}

#[derive(Clone)]
struct RegistryInner {
    acceptors: Arc<dyn RegistryOps>,
}

trait RegistryOps: Send + Sync {
    fn unregister(&self, acceptor_id: VarInt);
}

impl<T: Send + 'static> RegistryOps for DashMap<VarInt, Arc<dyn Acceptor<T>>, FxBuildHasher> {
    fn unregister(&self, acceptor_id: VarInt) {
        self.remove(&acceptor_id);
    }
}

/// Registry for managing acceptors
pub struct Registry<T: Send + 'static> {
    acceptors: Arc<DashMap<VarInt, Arc<dyn Acceptor<T>>, FxBuildHasher>>,
}

impl<T: Send + 'static> Clone for Registry<T> {
    fn clone(&self) -> Self {
        Self {
            acceptors: self.acceptors.clone(),
        }
    }
}

impl<T: Send + 'static> Registry<T> {
    /// Create a new acceptor registry
    pub fn new() -> Self {
        Self {
            acceptors: Arc::new(DashMap::with_hasher(FxBuildHasher::default())),
        }
    }

    /// Register a new acceptor and return a handle
    ///
    /// Returns None if the acceptor_id is already registered.
    pub fn register(&self, acceptor_id: VarInt, acceptor: Arc<dyn Acceptor<T>>) -> Option<Handle> {
        use dashmap::mapref::entry::Entry;

        // Try to insert, return None if already exists
        match self.acceptors.entry(acceptor_id) {
            Entry::Vacant(e) => {
                e.insert(acceptor);
                Some(Handle {
                    acceptor_id,
                    registry: RegistryInner {
                        acceptors: self.acceptors.clone() as Arc<dyn RegistryOps>,
                    },
                })
            }
            Entry::Occupied(_) => None,
        }
    }

    /// Looks up an acceptor by ID and runs the closure with a borrowed acceptor.
    ///
    /// `DashMap` uses sharded locking, so read-heavy workloads can run many
    /// lookups concurrently with minimal contention.
    pub fn with_acceptor<R>(
        &self,
        acceptor_id: VarInt,
        f: impl FnOnce(&dyn Acceptor<T>) -> R,
    ) -> Option<R> {
        let acceptor = self.acceptors.get(&acceptor_id)?;
        Some(f(acceptor.value().as_ref()))
    }
}

impl<T> Default for Registry<T>
where
    T: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct TestAcceptor {
        received: Mutex<Vec<String>>,
    }

    impl TestAcceptor {
        fn shared() -> Arc<Self> {
            Arc::new(Self {
                received: Mutex::new(Vec::new()),
            })
        }
    }

    impl Acceptor<String> for TestAcceptor {
        fn handle_request(&self, request: String) -> Result<AutoWake, Reject<String>> {
            self.received.lock().unwrap().push(request);
            Ok(AutoWake::new(None))
        }
    }

    #[tokio::test]
    async fn test_register_and_lookup() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        // Register acceptor
        let _handle = registry.register(acceptor_id, acceptor.clone()).unwrap();

        let acceptor2 = TestAcceptor::shared();

        // Cannot register same ID twice
        assert!(registry.register(acceptor_id, acceptor2).is_none());
    }

    #[tokio::test]
    async fn test_with_acceptor_not_found() {
        let registry: Registry<String> = Registry::new();

        let result = registry.with_acceptor(VarInt::from_u8(99), |_| ());

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_with_acceptor_and_handle() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        // Register acceptor
        let _handle = registry.register(acceptor_id, acceptor.clone()).unwrap();

        // Dispatch a request
        registry
            .with_acceptor(acceptor_id, |acceptor| {
                acceptor.handle_request("test".to_string()).unwrap();
            })
            .unwrap();

        // Verify the request was handled
        assert_eq!(*acceptor.received.lock().unwrap(), vec!["test"]);
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        // Register then drop handle (auto-unregister)
        {
            let _handle = registry.register(acceptor_id, acceptor).unwrap();
        }

        // Should no longer be found
        let result = registry.with_acceptor(acceptor_id, |_| ());
        assert!(result.is_none());
    }

    struct CustomPendingAcceptor;

    impl Acceptor<String> for CustomPendingAcceptor {
        fn handle_request(&self, _request: String) -> Result<AutoWake, Reject<String>> {
            Ok(AutoWake::new(None))
        }

        fn handle_pending(&self, request: String) -> Result<Dispatch, Reject<String>> {
            let action = if request == "accept" {
                PendingAction::Accepted
            } else {
                PendingAction::AcceptedWithRetry
            };
            Ok(Dispatch {
                action,
                waker: AutoWake::new(None),
            })
        }
    }

    #[tokio::test]
    async fn test_handle_pending() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let _handle = registry
            .register(acceptor_id, Arc::new(CustomPendingAcceptor))
            .unwrap();

        // Test accepted without retry
        let result = registry
            .with_acceptor(acceptor_id, |acceptor| {
                acceptor.handle_pending("accept".to_string())
            })
            .unwrap()
            .unwrap();
        assert!(matches!(result.action, PendingAction::Accepted));

        // Test accepted with retry
        let result = registry
            .with_acceptor(acceptor_id, |acceptor| {
                acceptor.handle_pending("retry".to_string())
            })
            .unwrap()
            .unwrap();
        assert!(matches!(result.action, PendingAction::AcceptedWithRetry));
    }

    #[tokio::test]
    async fn test_handle_pending_not_found() {
        let registry: Registry<String> = Registry::new();

        let result = registry.with_acceptor(VarInt::from_u8(99), |acceptor| {
            acceptor.handle_pending("test".to_string())
        });
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_default_pending_behavior() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        let _handle = registry.register(acceptor_id, acceptor).unwrap();

        // Default behavior should reject
        let result = registry
            .with_acceptor(acceptor_id, |acceptor| {
                acceptor.handle_pending("test".to_string())
            })
            .unwrap();
        assert!(result.is_err());
    }
}
