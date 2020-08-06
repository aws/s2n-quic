pub mod rx;
pub mod tx;

/// A pair of Rx and Tx IO implementations
#[derive(Debug)]
pub struct Pair<Rx, Tx> {
    pub rx: Rx,
    pub tx: Tx,
}

impl<'a, Rx: rx::Rx<'a>, Tx> rx::Rx<'a> for Pair<Rx, Tx> {
    type Queue = Rx::Queue;
    type Error = Rx::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.rx.queue()
    }

    fn len(&self) -> usize {
        self.rx.len()
    }

    fn receive(&mut self) -> Result<usize, Self::Error> {
        self.rx.receive()
    }
}

impl<'a, Rx, Tx: tx::Tx<'a>> tx::Tx<'a> for Pair<Rx, Tx> {
    type Queue = Tx::Queue;
    type Error = Tx::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.tx.queue()
    }

    fn len(&self) -> usize {
        self.tx.len()
    }

    fn transmit(&mut self) -> Result<usize, Self::Error> {
        self.tx.transmit()
    }
}
