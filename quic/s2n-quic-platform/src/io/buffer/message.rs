use core::ops::{Index, IndexMut};

pub trait MessageBuffer: Index<usize, Output = [u8]> + IndexMut<usize> {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn slot_size(&self) -> usize;
}
