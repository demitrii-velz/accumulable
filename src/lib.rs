pub mod iter;
pub mod stream;

pub trait Accumulable<Rhs = Self> {
    fn accumulate_from(&mut self, rhs: &Rhs);

    #[inline]
    fn accumulate(mut self, rhs: &Rhs) -> Self
    where
        Self: Sized,
    {
        self.accumulate_from(rhs);

        self
    }
}

pub trait MaybeAccumulable<Rhs = Self> {
    fn maybe_accumulate_from(&mut self, rhs: &Rhs) -> bool;

    #[inline]
    fn maybe_accumulate(mut self, rhs: &Rhs) -> Result<Self, Self>
    where
        Self: Sized,
    {
        if self.maybe_accumulate_from(rhs) {
            Ok(self)
        } else {
            Err(self)
        }
    }
}
