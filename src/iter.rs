use std::iter::Peekable;
use std::marker::PhantomData;

pub trait Neutral {
    fn neutral() -> Self;
}

pub trait Accumulable<Rhs = Self> {
    fn accumulate_in(&mut self, rhs: &Rhs);

    #[inline]
    fn accumulate(mut self, rhs: &Rhs) -> Self
    where
        Self: Sized,
    {
        self.accumulate_in(rhs);

        self
    }
}

pub trait Accumulate<Rhs> {
    fn accumulate<Lhs>(self) -> Lhs
    where
        Lhs: Neutral + Accumulable<Rhs>;
}

impl<I> Accumulate<I::Item> for I
where
    I: Iterator,
{
    #[inline]
    fn accumulate<Lhs>(self) -> Lhs
    where
        Lhs: Neutral + Accumulable<I::Item>,
    {
        let initial = Lhs::neutral();

        self.fold(initial, |lhs, rhs| lhs.accumulate(&rhs)).into()
    }
}

pub trait MaybeAccumulable<Rhs = Self> {
    fn maybe_accumulate_in(&mut self, rhs: &Rhs) -> bool;

    #[inline]
    fn maybe_accumulate(mut self, rhs: &Rhs) -> Result<Self, Self>
    where
        Self: Sized,
    {
        if self.maybe_accumulate_in(rhs) {
            Ok(self)
        } else {
            Err(self)
        }
    }
}

pub trait PartialAccumulate<Rhs>: Iterator {
    fn partial_accumulate<Lhs>(self) -> PartialAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: Neutral + MaybeAccumulable<Rhs>;
}

impl<I, Rhs> PartialAccumulate<Rhs> for I
where
    I: Iterator,
{
    #[inline]
    fn partial_accumulate<Lhs>(self) -> PartialAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: Neutral + MaybeAccumulable<Rhs>,
    {
        PartialAccumulated::new(self)
    }
}

pub struct PartialAccumulated<I, Lhs>
where
    I: Iterator,
{
    iter: Peekable<I>,
    _lhs: PhantomData<Lhs>,
}

impl<I, Lhs> PartialAccumulated<I, Lhs>
where
    I: Iterator,
    Lhs: Neutral,
{
    pub fn new(iter: I) -> Self {
        Self {
            iter: iter.peekable(),
            _lhs: PhantomData,
        }
    }
}

impl<I, Lhs> Iterator for PartialAccumulated<I, Lhs>
where
    I: Iterator,
    Lhs: From<I::Item> + MaybeAccumulable<I::Item>,
{
    type Item = Lhs;

    fn next(&mut self) -> Option<Self::Item> {
        let mut lhs = Lhs::from(self.iter.next()?);

        while let Some(rhs) = self.iter.peek() {
            if lhs.maybe_accumulate_in(rhs) {
                drop(self.iter.next());
            } else {
                break;
            }
        }

        Some(lhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Volume(u64);

    impl Neutral for Volume {
        fn neutral() -> Self {
            Volume(0)
        }
    }

    impl Accumulable for Volume {
        fn accumulate_in(&mut self, rhs: &Self) {
            *self = Volume(self.0 + rhs.0);
        }
    }

    #[test]
    fn test_accumulate() {
        let volumes = [Volume(10), Volume(15), Volume(20), Volume(25), Volume(30)];

        assert_eq!(volumes.into_iter().accumulate::<Volume>(), Volume(100));
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum VolumeSize<const N: u64> {
        Large(Volume),
        Small(Volume),
    }

    impl<const N: u64> VolumeSize<N> {
        pub fn new(volume: Volume) -> Self {
            if volume.0 >= N {
                Self::Large(volume)
            } else {
                Self::Small(volume)
            }
        }

        pub fn volume_value(&self) -> Volume {
            match self {
                Self::Large(x) | Self::Small(x) => *x,
            }
        }
    }

    impl<const N: u64> Neutral for VolumeSize<N> {
        fn neutral() -> Self {
            VolumeSize::Small(Volume::neutral())
        }
    }

    impl<const N: u64> Accumulable for VolumeSize<N> {
        fn accumulate_in(&mut self, rhs: &Self) {
            *self = VolumeSize::new(self.volume_value().accumulate(&rhs.volume_value()))
        }
    }

    impl<const N: u64> MaybeAccumulable for VolumeSize<N> {
        fn maybe_accumulate_in(&mut self, rhs: &Self) -> bool {
            match self {
                VolumeSize::Small(_) => {
                    self.accumulate_in(rhs);

                    true
                }
                _ => false,
            }
        }
    }

    #[test]
    fn partial_accumulate() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = [
            VolumeSize100::new(Volume(60)),
            VolumeSize100::new(Volume(30)),
            VolumeSize100::new(Volume(15)),
            //
            VolumeSize100::new(Volume(40)),
            VolumeSize100::new(Volume(70)),
            //
            VolumeSize100::new(Volume(80)),
            VolumeSize100::new(Volume(10)),
            VolumeSize100::new(Volume(5)),
            VolumeSize100::new(Volume(3)),
            VolumeSize100::new(Volume(2)),
            //
            VolumeSize100::new(Volume(1)),
            VolumeSize100::new(Volume(1)),
        ];

        let partial_accumulated = volumes
            .into_iter()
            .partial_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>();

        assert_eq!(
            partial_accumulated,
            vec![
                VolumeSize100::Large(Volume(105)),
                VolumeSize100::Large(Volume(110)),
                VolumeSize100::Large(Volume(100)),
                VolumeSize100::Small(Volume(2))
            ]
        )
    }
}
