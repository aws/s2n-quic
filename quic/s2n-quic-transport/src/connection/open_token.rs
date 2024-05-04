// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::num::NonZeroU64;

/// An opaque token issued to each connection handle which allows the stream
/// controller to track any pending open requests.
///
/// Each connection handle must have a unique instance of a token so their wakers are correctly
/// tracked.
#[derive(Debug)]
pub struct Pair {
    /// Stores the token for the unidirectional stream type
    pub(crate) unidirectional: Token,
    /// Stores the token for the bididirectional stream type
    pub(crate) bidirectional: Token,
}

impl Pair {
    /// Creates a new open token
    ///
    /// This should be held on the connection handle and should be presented each time
    /// `poll_open_stream` is called.
    #[inline]
    pub const fn new() -> Self {
        Self {
            unidirectional: Token::new(),
            bidirectional: Token::new(),
        }
    }
}

impl Default for Pair {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub(crate) struct Token(Option<NonZeroU64>);

impl Token {
    #[inline]
    pub const fn new() -> Self {
        Self(None)
    }

    /// Returns the index that the caller's waker should be stored, given the expired token
    #[inline]
    pub fn index(&self, expired_token: &Self) -> Option<usize> {
        let base = expired_token.0.map_or(0, |v| v.get());
        // since we're dealing with NonZeroU64, add one to get the correct index
        let base = base + 1;
        let v = self.0?;
        let v = v.get().checked_sub(base)?;
        Some(v as usize)
    }

    /// Expires a `count` number of tokens
    ///
    /// This should be called each time at least one waker is woken and removed from the waker list
    #[inline]
    pub fn expire(&mut self, count: usize) {
        if let Some(v) = self.0.as_mut() {
            *v = unsafe {
                // Safety: non-zero N + count is always non-zero
                NonZeroU64::new_unchecked(v.get() + count as u64)
            };
        } else if let Some(v) = NonZeroU64::new(count as _) {
            *self = Self(Some(v));
        }
    }

    /// Resets the token state
    #[inline]
    pub fn clear(&mut self) {
        *self = Self(None);
    }
}

#[derive(Debug)]
pub(crate) struct Counter(NonZeroU64);

impl Counter {
    pub const fn new() -> Self {
        Self(unsafe {
            // Safety: 1 is always non-zero
            NonZeroU64::new_unchecked(1)
        })
    }

    /// Returns the next open token for a connection
    #[inline]
    pub fn next(&mut self) -> Token {
        let v = self.0;
        let next = Token(Some(v));
        self.0 = unsafe {
            // Safety: N + 1 is always non-zero
            NonZeroU64::new_unchecked(v.get() + 1)
        };
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_token_test() {
        let mut caller_tokens = [Token::new(), Token::new(), Token::new()];
        let mut expired_token = Token::new();
        let mut token_counter = Counter::new();

        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "empty caller token with no expired tokens"
        );

        // give the caller a token
        caller_tokens[0] = token_counter.next();
        assert_eq!(
            caller_tokens[0].index(&expired_token),
            Some(0),
            "issued caller with no expired tokens should be index 0"
        );

        // expire the issued token
        expired_token.expire(1);
        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "issued caller token should now be expired"
        );

        for (idx, token) in caller_tokens.iter_mut().enumerate() {
            *token = token_counter.next();
            assert_eq!(
                token.index(&expired_token),
                Some(idx),
                "issued caller should be indexed in order",
            );
        }

        // expire just the first token
        expired_token.expire(1);

        assert_eq!(
            caller_tokens[0].index(&expired_token),
            None,
            "first caller token should now be expired"
        );

        for (idx, token) in caller_tokens[1..].iter().enumerate() {
            assert_eq!(
                token.index(&expired_token),
                Some(idx),
                "caller {} should not be expired",
                idx + 1,
            );
        }
    }
}
