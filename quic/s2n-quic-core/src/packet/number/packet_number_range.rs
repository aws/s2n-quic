use crate::packet::number::PacketNumber;

/// An inclusive range of `PacketNumber`s
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PacketNumberRange {
    pub start: PacketNumber,
    pub end: PacketNumber,
    current: Option<PacketNumber>,
}

impl PacketNumberRange {
    pub fn new(start: PacketNumber, end: PacketNumber) -> Self {
        assert!(start <= end, "start must be less than or equal to end");
        Self {
            start,
            end,
            current: None,
        }
    }
}

impl Iterator for PacketNumberRange {
    type Item = PacketNumber;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.current {
            if current < self.end {
                self.current = current.next();
                self.current
            } else {
                None
            }
        } else {
            self.current = Some(self.start);
            self.current
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        packet::number::{PacketNumberRange, PacketNumberSpace},
        varint::VarInt,
    };

    #[test]
    fn iterator() {
        let mut counter = 1;
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));

        for packet_number in PacketNumberRange::new(start, end) {
            assert_eq!(counter, packet_number.as_u64());
            counter += 1;
        }

        assert_eq!(counter, 11);
    }

    #[test]
    fn start_equals_end() {
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));

        let range = PacketNumberRange::new(start, end);

        assert_eq!(1, range.count());
        assert_eq!(start, range.last().unwrap());
    }

    #[test]
    #[should_panic]
    fn start_greater_than_end() {
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));
        PacketNumberRange::new(end, start);
    }
}
