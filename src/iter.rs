use std::iter::Peekable;
use std::marker::PhantomData;

use crate::{Accumulable, MaybeAccumulable};

pub trait Accumulate<Rhs> {
    fn accumulate<Lhs>(self) -> Option<Lhs>
    where
        Lhs: From<Rhs> + Accumulable<Rhs>;
}

impl<I> Accumulate<I::Item> for I
where
    I: Iterator,
{
    #[inline]
    fn accumulate<Lhs>(mut self) -> Option<Lhs>
    where
        Lhs: From<I::Item> + Accumulable<I::Item>,
    {
        let initial = Lhs::from(self.next()?);

        self.fold(initial, |lhs, rhs| lhs.accumulate(&rhs)).into()
    }
}

pub trait PartiallyAccumulate<Rhs>: Iterator {
    fn partially_accumulate<Lhs>(self) -> PartiallyAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>;
}

impl<I, Rhs> PartiallyAccumulate<Rhs> for I
where
    I: Iterator,
{
    #[inline]
    fn partially_accumulate<Lhs>(self) -> PartiallyAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>,
    {
        PartiallyAccumulated::new(self)
    }
}

pub struct PartiallyAccumulated<I, Lhs>
where
    I: Iterator,
{
    iter: Peekable<I>,
    _lhs: PhantomData<Lhs>,
}

impl<I, Lhs> PartiallyAccumulated<I, Lhs>
where
    I: Iterator,
{
    pub fn new(iter: I) -> Self {
        Self {
            iter: iter.peekable(),
            _lhs: PhantomData,
        }
    }
}

impl<I, Lhs> Iterator for PartiallyAccumulated<I, Lhs>
where
    I: Iterator,
    Lhs: From<I::Item> + MaybeAccumulable<I::Item>,
{
    type Item = Lhs;

    fn next(&mut self) -> Option<Self::Item> {
        let mut lhs = Lhs::from(self.iter.next()?);

        while let Some(rhs) = self.iter.peek() {
            if lhs.maybe_accumulate_from(rhs) {
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

    impl Accumulable for Volume {
        fn accumulate_from(&mut self, rhs: &Self) {
            *self = Volume(self.0 + rhs.0);
        }
    }

    #[test]
    fn test_accumulate_zero() {
        let volumes = [];

        assert_eq!(volumes.into_iter().accumulate::<Volume>(), None);
    }

    #[test]
    fn test_accumulate_one() {
        let volumes = [Volume(10)];

        assert_eq!(
            volumes.into_iter().accumulate::<Volume>().unwrap(),
            Volume(10)
        );
    }

    #[test]
    fn test_accumulate() {
        let volumes = [Volume(10), Volume(15), Volume(20), Volume(25), Volume(30)];

        assert_eq!(
            volumes.into_iter().accumulate::<Volume>().unwrap(),
            Volume(100)
        );
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

    impl<const N: u64> Accumulable for VolumeSize<N> {
        fn accumulate_from(&mut self, rhs: &Self) {
            *self = VolumeSize::new(self.volume_value().accumulate(&rhs.volume_value()))
        }
    }

    impl<const N: u64> MaybeAccumulable for VolumeSize<N> {
        fn maybe_accumulate_from(&mut self, rhs: &Self) -> bool {
            match self {
                VolumeSize::Small(_) => {
                    self.accumulate_from(rhs);

                    true
                }
                _ => false,
            }
        }
    }

    #[test]
    fn partially_accumulate_zero() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = [];

        let partially_accumulated = volumes
            .into_iter()
            .partially_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>();

        assert_eq!(partially_accumulated, vec![])
    }

    #[test]
    fn partially_accumulate_one() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = [VolumeSize100::new(Volume(60))];

        let partially_accumulated = volumes
            .into_iter()
            .partially_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>();

        assert_eq!(partially_accumulated, vec![VolumeSize100::Small(Volume(60))])
    }

    #[test]
    fn partially_accumulate() {
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

        let partially_accumulated = volumes
            .into_iter()
            .partially_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>();

        assert_eq!(
            partially_accumulated,
            vec![
                VolumeSize100::Large(Volume(105)),
                VolumeSize100::Large(Volume(110)),
                VolumeSize100::Large(Volume(100)),
                VolumeSize100::Small(Volume(2))
            ]
        )
    }
}
