// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::AckManager, contexts::WriteContext, endpoint, recovery, space::CryptoStream, transmission,
};
use core::ops::RangeInclusive;

pub struct Payload<'a, Config: endpoint::Config> {
    pub ack_manager: &'a mut AckManager,
    pub crypto_stream: &'a mut CryptoStream,
    pub recovery_manager: &'a mut recovery::Manager<Config>,
}

/// Rather than creating a packet with a very small CRYPTO frame (under 16 bytes), it would be
/// better to wait for another transmission and send something larger. This should be better for
/// performance, anyway, since you end up paying for encryption/decryption.
const MIN_SIZE: usize = 16;

impl<'a, Config: endpoint::Config> super::Payload for Payload<'a, Config> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        (*range.start()).max(MIN_SIZE)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        debug_assert!(
            !context.transmission_mode().is_mtu_probing(),
            "Early transmissions should not be used for MTU probing"
        );

        let did_send_ack = self.ack_manager.on_transmit(context);

        // Payloads can only transmit and retransmit
        if context.transmission_constraint().can_transmit()
            || context.transmission_constraint().can_retransmit()
        {
            let _ = self.crypto_stream.tx.on_transmit((), context);

            // send PINGs last, since they might not actually be needed if there's an ack-eliciting
            // frame already present in the payload
            self.recovery_manager.on_transmit(context);
        }

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(context);
        }
    }
}

impl<'a, Config: endpoint::Config> transmission::interest::Provider for Payload<'a, Config> {
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.ack_manager.transmission_interest(query)?;
        self.crypto_stream.transmission_interest(query)?;
        self.recovery_manager.transmission_interest(query)?;
        Ok(())
    }
}
