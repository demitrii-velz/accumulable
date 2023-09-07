use std::mem::replace;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project::pin_project;

use futures::future::{FusedFuture, Future};
use futures::ready;
use futures::stream::{FusedStream, Stream};

use crate::{Accumulable, MaybeAccumulable};

pub trait Accumulate<Rhs>: Sized {
    fn accumulate<Lhs>(self) -> Accumulated<Self, Lhs>
    where
        Lhs: From<Rhs> + Accumulable<Rhs>;
}

pub enum AccumulatedState<Lhs> {
    Uninit,
    Accumulable(Lhs),
    Consumed,
}

impl<Lhs> AccumulatedState<Lhs> {
    fn consume(&mut self) -> Option<Lhs> {
        match replace(self, AccumulatedState::Consumed) {
            AccumulatedState::Accumulable(item) => Some(item),
            _ => None,
        }
    }

    fn reaccumulable(&mut self, lhs: Lhs) -> Option<Lhs> {
        match replace(self, AccumulatedState::Accumulable(lhs)) {
            AccumulatedState::Accumulable(item) => Some(item),
            _ => None,
        }
    }

    fn reinit(&mut self) -> Option<Lhs> {
        match replace(self, AccumulatedState::Uninit) {
            AccumulatedState::Accumulable(item) => Some(item),
            _ => None,
        }
    }

    fn is_consumed(&self) -> bool {
        match self {
            AccumulatedState::Consumed => true,
            _ => false,
        }
    }
}

#[pin_project]
pub struct Accumulated<S, Lhs> {
    #[pin]
    stream: S,
    lhs: AccumulatedState<Lhs>,
}

impl<S, Lhs> FusedFuture for Accumulated<S, Lhs>
where
    S: Stream,
    Lhs: From<S::Item> + Accumulable<S::Item>,
{
    fn is_terminated(&self) -> bool {
        self.lhs.is_consumed()
    }
}

impl<S, Lhs> Future for Accumulated<S, Lhs>
where
    S: Stream,
    Lhs: From<S::Item> + Accumulable<S::Item>,
{
    type Output = Option<Lhs>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use AccumulatedState as S;

        let mut proj = self.project();

        let result = loop {
            match proj.lhs {
                S::Uninit => {
                    let first = ready!(proj.stream.as_mut().poll_next(cx));

                    match first {
                        Some(first) => {
                            *proj.lhs = AccumulatedState::Accumulable(Lhs::from(first));
                        }
                        None => break proj.lhs.consume(),
                    }
                }
                S::Accumulable(inner) => {
                    let item = ready!(proj.stream.as_mut().poll_next(cx));

                    match item {
                        Some(item) => {
                            inner.accumulate_from(&item);
                        }
                        None => {
                            drop(inner);

                            break proj.lhs.consume();
                        }
                    }
                }
                S::Consumed => panic!("Accumulated polled after completion"),
            }
        };

        Poll::Ready(result)
    }
}

impl<S> Accumulate<S::Item> for S
where
    S: Stream,
{
    #[inline]
    fn accumulate<Lhs>(self) -> Accumulated<S, Lhs>
    where
        Lhs: From<S::Item> + Accumulable<S::Item>,
    {
        Accumulated {
            stream: self,
            lhs: AccumulatedState::Uninit,
        }
    }
}

pub trait PartiallyAccumulate<Rhs> {
    fn partially_accumulate<Lhs>(self) -> PartiallyAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>;
}

impl<S, Rhs> PartiallyAccumulate<Rhs> for S {
    #[inline]
    fn partially_accumulate<Lhs>(self) -> PartiallyAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>,
    {
        PartiallyAccumulated::new(self)
    }
}

#[pin_project]
pub struct PartiallyAccumulated<S, Lhs> {
    #[pin]
    stream: S,
    lhs: AccumulatedState<Lhs>,
}

impl<S, Lhs> PartiallyAccumulated<S, Lhs> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            lhs: AccumulatedState::Uninit,
        }
    }
}

impl<S, Lhs> FusedStream for PartiallyAccumulated<S, Lhs>
where
    S: Stream,
    Lhs: From<S::Item> + MaybeAccumulable<S::Item>,
{
    fn is_terminated(&self) -> bool {
        self.lhs.is_consumed()
    }
}

impl<S, Lhs> Stream for PartiallyAccumulated<S, Lhs>
where
    S: Stream,
    Lhs: From<S::Item> + MaybeAccumulable<S::Item>,
{
    type Item = Lhs;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use AccumulatedState as S;

        let mut proj = self.project();

        let result = loop {
            match proj.lhs {
                S::Uninit => {
                    let first = ready!(proj.stream.as_mut().poll_next(cx));

                    match first {
                        Some(first) => {
                            *proj.lhs = AccumulatedState::Accumulable(Lhs::from(first));
                        }
                        None => break proj.lhs.consume(),
                    }
                }
                S::Accumulable(inner) => {
                    let item = ready!(proj.stream.as_mut().poll_next(cx));

                    match item {
                        Some(item) => {
                            if inner.maybe_accumulate_from(&item) {
                                continue;
                            } else {
                                drop(inner);

                                break proj.lhs.reaccumulable(Lhs::from(item));
                            }
                        }
                        None => {
                            drop(inner);

                            break proj.lhs.reinit();
                        }
                    }
                }
                S::Consumed => panic!("Accumulated polled after completion"),
            }
        };

        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use futures::stream::{self, StreamExt};

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Volume(u64);

    impl Accumulable for Volume {
        fn accumulate_from(&mut self, rhs: &Self) {
            *self = Volume(self.0 + rhs.0);
        }
    }

    #[tokio::test]
    async fn test_accumulate_zero() {
        let volumes = stream::iter([]);

        assert_eq!(volumes.accumulate::<Volume>().await, None);
    }

    #[tokio::test]
    async fn test_accumulate_one() {
        let volumes = stream::iter([Volume(10)]);

        assert_eq!(volumes.accumulate::<Volume>().await.unwrap(), Volume(10));
    }

    #[tokio::test]
    async fn test_accumulate() {
        let volumes = stream::iter([Volume(10), Volume(15), Volume(20), Volume(25), Volume(30)]);

        assert_eq!(volumes.accumulate::<Volume>().await.unwrap(), Volume(100));
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

    #[tokio::test]
    async fn partially_accumulate_zero() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = stream::iter([]);

        let partially_accumulated = volumes
            .partially_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>()
            .await;

        assert_eq!(partially_accumulated, vec![])
    }

    #[tokio::test]
    async fn partially_accumulate_one() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = stream::iter([VolumeSize100::new(Volume(60))]);

        let partially_accumulated = volumes
            .partially_accumulate::<VolumeSize100>()
            .fuse()
            .collect::<Vec<_>>()
            .await;

        assert_eq!(partially_accumulated, vec![VolumeSize100::Small(Volume(60))])
    }

    #[tokio::test]
    async fn partially_accumulate() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = stream::iter([
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
        ]);

        let partially_accumulated = volumes
            .partially_accumulate::<VolumeSize100>()
            .collect::<Vec<_>>()
            .await;

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
