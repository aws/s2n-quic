use super::Segment;
use crate::message;

pub trait Behavior {
    fn advance<Message: message::Message>(
        &self,
        messages: &mut [Message],
        segment: &Segment,
        count: usize,
    );

    fn cancel<Message: message::Message>(
        &self,
        messages: &mut [Message],
        segment: &Segment,
        count: usize,
    );
}

#[derive(Debug)]
pub struct Occupied {
    pub(crate) mtu: usize,
}

impl Occupied {
    #[inline]
    fn reset<Message: message::Message>(&self, messages: &mut [Message]) {
        for message in messages {
            unsafe {
                message.set_payload_len(self.mtu);
                message.reset_remote_address();
            }
        }
    }

    #[inline]
    fn wipe<Message: message::Message>(&self, messages: &mut [Message]) {
        for message in messages {
            #[cfg(feature = "wipe")]
            zeroize::Zeroize::zeroize(&mut message.payload_mut().iter_mut());

            unsafe {
                message.set_payload_len(self.mtu);
                message.reset_remote_address();
            }
        }
    }
}

impl Behavior for Occupied {
    fn advance<Message: message::Message>(
        &self,
        messages: &mut [Message],
        segment: &Segment,
        count: usize,
    ) {
        let capacity = segment.capacity;
        let prev_index = segment.index;
        let new_index = prev_index + count;

        let start = prev_index;
        let end = new_index.min(capacity);
        let overflow = new_index.saturating_sub(capacity);

        let (primary, secondary) = messages.split_at_mut(capacity);

        self.wipe(&mut primary[start..end]);
        self.wipe(&mut primary[..overflow]);
        self.reset(&mut secondary[start..end]);
        self.reset(&mut secondary[..overflow]);
    }

    fn cancel<Message: message::Message>(
        &self,
        _messages: &mut [Message],
        _segment: &Segment,
        _count: usize,
    ) {
    }
}

#[derive(Debug)]
pub struct Free {
    pub(crate) mtu: usize,
}

impl Free {
    fn replicate<Message: message::Message>(&self, from: &mut [Message], to: &mut [Message]) {
        for (from_msg, to_msg) in from.iter_mut().zip(to.iter_mut()) {
            to_msg.replicate_fields_from(from_msg);
        }
    }
}

impl Behavior for Free {
    fn advance<Message: message::Message>(
        &self,
        messages: &mut [Message],
        segment: &Segment,
        count: usize,
    ) {
        let capacity = segment.capacity;
        let prev_index = segment.index;
        let new_index = prev_index + count;

        let start = prev_index;
        let end = new_index.min(capacity);
        let overflow = new_index.saturating_sub(capacity);

        // copy the overflowed message field lengths to the primary messages
        let (primary, secondary) = messages.split_at_mut(capacity);

        self.replicate(&mut secondary[..overflow], &mut primary[..overflow]);
        self.replicate(&mut primary[start..end], &mut secondary[start..end]);
    }

    fn cancel<Message: message::Message>(
        &self,
        messages: &mut [Message],
        segment: &Segment,
        count: usize,
    ) {
        Occupied { mtu: self.mtu }.advance(messages, segment, count)
    }
}
