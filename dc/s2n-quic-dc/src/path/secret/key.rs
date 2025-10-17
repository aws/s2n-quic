// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{map, schedule};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use s2n_quic_core::{packet::KeyPhase, time::Clock};

pub mod seal {
    use super::*;
    use crate::crypto::{awslc, seal};

    use crate::{event, event::ConnectionPublisher, stream::shared};
    pub use awslc::seal::control;
    use s2n_quic_core::event::IntoEvent;

    pub const TEST_MAX_RECORDS: u64 = 4096;

    #[derive(Debug)]
    pub struct Application {
        sealer: awslc::seal::Application,
        ku: schedule::SealUpdate,
        key_phase: KeyPhase,
        encrypted_records: AtomicU64,
    }

    impl Application {
        #[inline]
        pub(crate) fn new(sealer: awslc::seal::Application, ku: schedule::SealUpdate) -> Self {
            Self {
                sealer,
                ku,
                key_phase: KeyPhase::Zero,
                encrypted_records: AtomicU64::new(0),
            }
        }

        #[inline]
        pub fn needs_update(&self) -> bool {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
            //# For AEAD_AES_128_GCM and AEAD_AES_256_GCM, the confidentiality limit
            //# is 2^23 encrypted packets; see Appendix B.1.
            const LIMIT: u64 = 2u64.pow(23);

            // enqueue key updates 2^16 packets before the limit is hit
            const THRESHOLD: u64 = 2u64.pow(16);

            // in debug mode, rotate keys more often in order to surface any issues
            const MAX_RECORDS: u64 = if cfg!(debug_assertions) {
                TEST_MAX_RECORDS
            } else {
                LIMIT - THRESHOLD
            };

            self.encrypted_records.load(Ordering::Relaxed) >= MAX_RECORDS
        }

        #[inline]
        pub fn update<C: Clock + ?Sized, Sub: event::Subscriber>(
            &mut self,
            clock: &C,
            subscriber: &shared::Subscriber<Sub>,
        ) {
            let (sealer, ku) = self.ku.next();
            self.sealer = sealer;
            self.ku = ku;
            self.encrypted_records = AtomicU64::new(0);
            self.key_phase = self.key_phase.next_phase();
            tracing::debug!(sealer_updated = ?self.key_phase);

            subscriber
                .publisher(clock.get_time())
                .on_stream_write_key_updated(event::builder::StreamWriteKeyUpdated {
                    key_phase: self.key_phase.into_event(),
                })
        }
    }

    impl seal::Application for Application {
        #[inline]
        fn key_phase(&self) -> KeyPhase {
            self.key_phase
        }

        #[inline]
        fn tag_len(&self) -> usize {
            self.sealer.tag_len()
        }

        #[inline]
        fn encrypt(
            &self,
            packet_number: u64,
            header: &[u8],
            extra_payload: Option<&[u8]>,
            payload_and_tag: &mut [u8],
        ) {
            self.encrypted_records.fetch_add(1, Ordering::Relaxed);
            self.sealer
                .encrypt(packet_number, header, extra_payload, payload_and_tag)
        }
    }

    #[derive(Debug)]
    pub struct Once {
        key: awslc::seal::Application,
        sealed: AtomicBool,
    }

    impl Once {
        pub(crate) fn new(key: awslc::seal::Application) -> Self {
            Self {
                key,
                sealed: AtomicBool::new(false),
            }
        }
    }

    impl seal::Application for Once {
        #[inline]
        fn key_phase(&self) -> KeyPhase {
            KeyPhase::Zero
        }

        #[inline]
        fn tag_len(&self) -> usize {
            self.key.tag_len()
        }

        #[inline]
        fn encrypt(
            &self,
            packet_number: u64,
            header: &[u8],
            extra_payload: Option<&[u8]>,
            payload_and_tag: &mut [u8],
        ) {
            assert!(!self.sealed.swap(true, Ordering::Relaxed));
            self.key
                .encrypt(packet_number, header, extra_payload, payload_and_tag)
        }
    }
}

pub mod open {
    use super::*;
    use crate::crypto::{awslc, open, UninitSlice};
    use core::mem::MaybeUninit;
    use s2n_quic_core::ensure;
    use zeroize::Zeroize;

    use crate::{event, event::ConnectionPublisher, stream::shared};
    pub use awslc::open::control;
    use s2n_quic_core::event::IntoEvent;

    macro_rules! with_dedup {
        () => {
            /// Disables replay prevention allowing the decryption key to be reused.
            ///
            /// ## Safety
            /// Disabling replay prevention is insecure because it makes it possible for
            /// active network attackers to cause a peer to accept previously processed
            /// data as new. For example, if a packet contains a mutating request such
            /// as adding +1 to a value in a database, an attacker can keep replaying
            /// packets to increment the value beyond what the original legitimate
            /// sender of the packet intended.
            pub unsafe fn disable_replay_prevention(&mut self) {
                self.dedup.disable();
            }

            /// Ensures the key has not been used before
            #[inline]
            pub fn on_decrypt_success(&self, payload: &mut UninitSlice) -> open::Result {
                self.dedup.check().map_err(|e| {
                    let payload = unsafe {
                        let ptr = payload.as_mut_ptr() as *mut MaybeUninit<u8>;
                        let len = payload.len();
                        core::slice::from_raw_parts_mut(ptr, len)
                    };
                    payload.zeroize();
                    e
                })?;

                Ok(())
            }

            #[doc(hidden)]
            #[cfg(any(test, feature = "testing"))]
            pub fn dedup_check(&self) -> open::Result {
                self.dedup.check()
            }
        };
    }

    #[derive(Debug)]
    pub struct Application {
        openers: [awslc::open::Application; 2],
        ku: schedule::OpenUpdate,
        // the current expected phase
        key_phase: KeyPhase,
        dedup: map::Dedup,
        needs_update: AtomicBool,
    }

    impl Application {
        pub(crate) fn new(
            opener: awslc::open::Application,
            ku: schedule::OpenUpdate,
            dedup: map::Dedup,
        ) -> Self {
            let (opener2, ku) = ku.next();
            let openers = [opener, opener2];
            Self {
                openers,
                ku,
                key_phase: KeyPhase::Zero,
                dedup,
                needs_update: AtomicBool::new(false),
            }
        }

        with_dedup!();

        #[inline]
        pub fn needs_update(&self) -> bool {
            self.needs_update.load(Ordering::Relaxed)
        }

        #[inline]
        pub fn update<C: Clock + ?Sized, Sub: event::Subscriber>(
            &mut self,
            clock: &C,
            subscriber: &shared::Subscriber<Sub>,
        ) {
            let idx = match self.key_phase {
                KeyPhase::Zero => 0,
                KeyPhase::One => 1,
            };
            let (opener, ku) = self.ku.next();
            self.openers[idx] = opener;
            self.ku = ku;
            self.key_phase = self.key_phase.next_phase();
            self.needs_update.store(false, Ordering::Relaxed);
            tracing::debug!(opener_updated = ?self.key_phase);

            subscriber
                .publisher(clock.get_time())
                .on_stream_read_key_updated(event::builder::StreamReadKeyUpdated {
                    key_phase: self.key_phase.into_event(),
                })
        }
    }

    impl open::Application for Application {
        #[inline]
        fn tag_len(&self) -> usize {
            self.openers[0].tag_len()
        }

        #[inline]
        fn decrypt(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_in: &[u8],
            tag: &[u8],
            payload_out: &mut UninitSlice,
        ) -> open::Result {
            let opener = match key_phase {
                KeyPhase::Zero => &self.openers[0],
                KeyPhase::One => &self.openers[1],
            };

            opener.decrypt(
                // the underlying key doesn't perform rotation
                KeyPhase::Zero,
                packet_number,
                header,
                payload_in,
                tag,
                payload_out,
            )?;

            self.on_decrypt_success(payload_out)?;

            if key_phase != self.key_phase {
                self.needs_update.store(true, Ordering::Relaxed);
            }

            Ok(())
        }

        #[inline]
        fn decrypt_in_place(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_and_tag: &mut [u8],
        ) -> open::Result {
            let opener = match key_phase {
                KeyPhase::Zero => &self.openers[0],
                KeyPhase::One => &self.openers[1],
            };

            opener.decrypt_in_place(
                // the underlying key doesn't perform rotation
                KeyPhase::Zero,
                packet_number,
                header,
                payload_and_tag,
            )?;

            self.on_decrypt_success(payload_and_tag.into())?;

            if key_phase != self.key_phase {
                self.needs_update.store(true, Ordering::Relaxed);
            }

            Ok(())
        }
    }

    #[derive(Debug)]
    pub struct Once {
        key: awslc::open::Application,
        dedup: map::Dedup,
        opened: AtomicBool,
    }

    impl Once {
        pub(crate) fn new(key: awslc::open::Application, dedup: map::Dedup) -> Self {
            Self {
                key,
                dedup,
                opened: AtomicBool::new(false),
            }
        }

        with_dedup!();
    }

    impl open::Application for Once {
        #[inline]
        fn tag_len(&self) -> usize {
            self.key.tag_len()
        }

        #[inline]
        fn decrypt(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_in: &[u8],
            tag: &[u8],
            payload_out: &mut UninitSlice,
        ) -> open::Result {
            ensure!(
                key_phase == KeyPhase::Zero,
                Err(open::Error::RotationNotSupported)
            );

            self.key.decrypt(
                key_phase,
                packet_number,
                header,
                payload_in,
                tag,
                payload_out,
            )?;

            self.on_decrypt_success(payload_out)?;

            ensure!(
                !self.opened.swap(true, Ordering::Relaxed),
                Err(open::Error::SingleUseKey)
            );

            Ok(())
        }

        #[inline]
        fn decrypt_in_place(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_and_tag: &mut [u8],
        ) -> open::Result {
            ensure!(
                key_phase == KeyPhase::Zero,
                Err(open::Error::RotationNotSupported)
            );

            self.key
                .decrypt_in_place(key_phase, packet_number, header, payload_and_tag)?;

            self.on_decrypt_success(payload_and_tag.into())?;

            ensure!(
                !self.opened.swap(true, Ordering::Relaxed),
                Err(open::Error::SingleUseKey)
            );

            Ok(())
        }
    }
}
