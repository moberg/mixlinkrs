//! Single-producer / single-consumer queue wrappers. Thin passthroughs over
//! `rtrb` with convenience `drain_into` helpers.

use rtrb::{Consumer as RtConsumer, Producer as RtProducer, RingBuffer};

pub struct Producer<T: Send>(pub RtProducer<T>);
pub struct Consumer<T: Send>(pub RtConsumer<T>);

/// Create a bounded SPSC channel with capacity `cap`.
pub fn spsc_bounded<T: Send>(cap: usize) -> (Producer<T>, Consumer<T>) {
    let (p, c) = RingBuffer::<T>::new(cap);
    (Producer(p), Consumer(c))
}

impl<T: Send> Producer<T> {
    #[inline]
    pub fn try_push(&mut self, v: T) -> Result<(), T> {
        match self.0.push(v) {
            Ok(()) => Ok(()),
            Err(rtrb::PushError::Full(x)) => Err(x),
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.buffer().capacity()
    }
}

impl<T: Send> Consumer<T> {
    #[inline]
    pub fn try_pop(&mut self) -> Option<T> {
        self.0.pop().ok()
    }

    /// Drain up to `max` items, invoking `f` with each. Stops at first empty.
    pub fn drain<F: FnMut(T)>(&mut self, max: usize, mut f: F) -> usize {
        let mut n = 0;
        while n < max {
            match self.0.pop() {
                Ok(v) => {
                    f(v);
                    n += 1;
                }
                Err(_) => break,
            }
        }
        n
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.slots()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_order() {
        let (mut p, mut c) = spsc_bounded::<i32>(16);
        for i in 0..8 {
            p.try_push(i).unwrap();
        }
        let mut out = Vec::new();
        c.drain(8, |v| out.push(v));
        assert_eq!(out, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn full_returns_err() {
        let (mut p, _c) = spsc_bounded::<i32>(2);
        p.try_push(1).unwrap();
        p.try_push(2).unwrap();
        assert!(p.try_push(3).is_err());
    }
}
