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
//!     fn handle_request(&self, request: String) -> AutoWake {
//!         // Spawn a task to handle the request
//!         tokio::spawn(async move {
//!             println!("Handling request: {}", request);
//!         });
//!         AutoWake::new(None)
//!     }
//!
//!     fn handle_pending(&self, request: String) -> Dispatch {
//!         // For pending requests that can't be confirmed as non-duplicate,
//!         // accept them but request a retry to confirm
//!         println!("Handling pending request: {}", request);
//!         Dispatch {
//!             action: PendingAction::AcceptedWithRetry,
//!             waker: AutoWake::new(None),
//!         }
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
//! // Dispatch confirmed requests to the acceptor
//! registry.dispatch(VarInt::from_u8(1), "hello".to_string()).unwrap();
//!
//! // Dispatch pending requests that need deduplication handling
//! match registry
//!     .dispatch_pending(VarInt::from_u8(1), "pending".to_string())
//!     .unwrap()
//!     .action
//! {
//!     PendingAction::Accepted => println!("Accepted without retry"),
//!     PendingAction::AcceptedWithRetry => println!("Accepted, send retry request"),
//!     PendingAction::Reject { reset_code } => println!("Rejected with code {}", reset_code),
//! }
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

/// Trait for handling incoming requests
pub trait Acceptor<T>: Send + Sync + 'static {
    /// Called when a new request arrives.
    ///
    /// Returns an `AutoWake` that should be forwarded to the waker thread.
    fn handle_request(&self, request: T) -> AutoWake;

    /// Called when a request arrives but cannot be confirmed as non-duplicate.
    ///
    /// Returns a `Dispatch` indicating the action to take and an `AutoWake`.
    fn handle_pending(&self, _request: T) -> Dispatch {
        Dispatch {
            action: PendingAction::Reject {
                reset_code: VarInt::from_u32(0),
            },
            waker: AutoWake::new(None),
        }
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
    /// The acceptor rejected the request and a flow reset should be sent
    Reject { reset_code: VarInt },
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

    /// Dispatch a request to the specified acceptor
    ///
    /// Returns Err if the acceptor doesn't exist.
    pub fn dispatch(&self, acceptor_id: VarInt, request: T) -> Result<AutoWake, DispatchError> {
        let Some(acceptor) = self.acceptors.get(&acceptor_id) else {
            return Err(DispatchError::AcceptorNotFound);
        };

        Ok(acceptor.handle_request(request))
    }

    /// Dispatch a pending request that cannot be confirmed as non-duplicate
    ///
    /// Returns the action the acceptor wants taken, or Err if the acceptor doesn't exist.
    pub fn dispatch_pending(
        &self,
        acceptor_id: VarInt,
        request: T,
    ) -> Result<Dispatch, DispatchError> {
        let Some(acceptor) = self.acceptors.get(&acceptor_id) else {
            return Err(DispatchError::AcceptorNotFound);
        };

        Ok(acceptor.handle_pending(request))
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

/// Error when dispatching a request
#[derive(Debug)]
pub enum DispatchError {
    /// Acceptor with this ID does not exist
    AcceptorNotFound,
    /// The flow should be reset immediately with the given code
    Reset { reset_code: VarInt },
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
        fn handle_request(&self, request: String) -> AutoWake {
            self.received.lock().unwrap().push(request);
            AutoWake::new(None)
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
    async fn test_dispatch_not_found() {
        let registry: Registry<String> = Registry::new();

        let result = registry.dispatch(VarInt::from_u8(99), "test".to_string());

        assert!(matches!(result, Err(DispatchError::AcceptorNotFound)));
    }

    #[tokio::test]
    async fn test_dispatch_and_handle() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        // Register acceptor
        let _handle = registry.register(acceptor_id, acceptor.clone()).unwrap();

        // Dispatch a request
        registry.dispatch(acceptor_id, "test".to_string()).unwrap();

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

        // Should no longer be found - dispatch should fail
        let result = registry.dispatch(acceptor_id, "test".to_string());
        assert!(matches!(result, Err(DispatchError::AcceptorNotFound)));
    }

    struct CustomPendingAcceptor;

    impl Acceptor<String> for CustomPendingAcceptor {
        fn handle_request(&self, _request: String) -> AutoWake {
            AutoWake::new(None)
        }

        fn handle_pending(&self, request: String) -> Dispatch {
            let action = if request == "accept" {
                PendingAction::Accepted
            } else if request == "retry" {
                PendingAction::AcceptedWithRetry
            } else {
                PendingAction::Reject {
                    reset_code: VarInt::from_u32(42),
                }
            };
            Dispatch {
                action,
                waker: AutoWake::new(None),
            }
        }
    }

    #[tokio::test]
    async fn test_dispatch_pending() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let _handle = registry
            .register(acceptor_id, Arc::new(CustomPendingAcceptor))
            .unwrap();

        // Test accepted without retry
        let result = registry
            .dispatch_pending(acceptor_id, "accept".to_string())
            .unwrap();
        assert!(matches!(result.action, PendingAction::Accepted));

        // Test accepted with retry
        let result = registry
            .dispatch_pending(acceptor_id, "retry".to_string())
            .unwrap();
        assert!(matches!(result.action, PendingAction::AcceptedWithRetry));

        // Test rejection
        let result = registry
            .dispatch_pending(acceptor_id, "reject".to_string())
            .unwrap();
        assert!(
            matches!(result.action, PendingAction::Reject { reset_code } if reset_code == VarInt::from_u32(42))
        );
    }

    #[tokio::test]
    async fn test_dispatch_pending_not_found() {
        let registry: Registry<String> = Registry::new();

        let result = registry.dispatch_pending(VarInt::from_u8(99), "test".to_string());
        assert!(matches!(result, Err(DispatchError::AcceptorNotFound)));
    }

    #[tokio::test]
    async fn test_default_pending_behavior() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let acceptor = TestAcceptor::shared();

        let _handle = registry.register(acceptor_id, acceptor).unwrap();

        // Default behavior should reject
        let result = registry
            .dispatch_pending(acceptor_id, "test".to_string())
            .unwrap();
        assert!(matches!(result.action, PendingAction::Reject { .. }));
    }
}
