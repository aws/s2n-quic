use crate::packet::number::PacketNumber;

/// An inclusive range of `PacketNumber`s
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PacketNumberRange {
    pub start: PacketNumber,
    pub end: PacketNumber,
    exhausted: bool,
}

impl PacketNumberRange {
    pub fn new(start: PacketNumber, end: PacketNumber) -> Self {
        assert!(start <= end, "start must be less than or equal to end");
        Self {
            start,
            end,
            exhausted: false,
        }
    }
}

impl Iterator for PacketNumberRange {
    type Item = PacketNumber;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.exhausted && self.start <= self.end {
            let current = self.start;
            if let Some(next) = current.next() {
                self.start = next;
            } else {
                // PacketNumber range has been exceeded
                self.exhausted = true;
            }
            Some(current)
        } else {
            self.exhausted = true;
            None
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

    #[test]
    fn end_is_max_packet_number() {
        let start = PacketNumberSpace::Handshake.new_packet_number(
            VarInt::new(0b11111111111111111111111111111111111111111111111111111111111110).unwrap(),
        );
        let end = PacketNumberSpace::Handshake.new_packet_number(
            VarInt::new(0b11111111111111111111111111111111111111111111111111111111111111).unwrap(),
        );

        assert_eq!(2, PacketNumberRange::new(start, end).count());
    }
}
