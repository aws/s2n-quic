use crate::{buffer::Buffer, message::queue::Queue};
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(s2n_quic_platform_socket_mmsg)] {
        use crate::message::mmsg::Ring;
    } else if #[cfg(s2n_quic_platform_socket_msg)] {
        use crate::message::msg::Ring;
    }
}

pub type MessageQueue<B> = Queue<Ring<B>>;

pub fn new<B: Buffer>(buffer: B) -> MessageQueue<B> {
    MessageQueue::new(Ring::new(buffer))
}
