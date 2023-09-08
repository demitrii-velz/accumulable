use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project::pin_project;

use futures::ready;
use futures::stream::{FusedStream, Stream};

use crate::stream::AccumulatedState;
use crate::MaybeAccumulable;

pub trait TryPartiallyAccumulate<Rhs> {
    fn try_partially_accumulate<Lhs>(self) -> TryPartiallyAccumulated<Self, Lhs>
    where
        Self: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>;
}

impl<S, Rhs> TryPartiallyAccumulate<Rhs> for S
where
    S: Stream,
{
    #[inline]
    fn try_partially_accumulate<Lhs>(self) -> TryPartiallyAccumulated<S, Lhs>
    where
        S: Sized,
        Lhs: From<Rhs> + MaybeAccumulable<Rhs>,
    {
        TryPartiallyAccumulated::new(self)
    }
}

#[pin_project]
pub struct TryPartiallyAccumulated<S, Lhs> {
    #[pin]
    stream: S,
    lhs: AccumulatedState<Lhs>,
}

impl<S, Lhs> TryPartiallyAccumulated<S, Lhs> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            lhs: AccumulatedState::Uninit,
        }
    }
}

impl<S, Lhs, V, E> FusedStream for TryPartiallyAccumulated<S, Lhs>
where
    S: Stream<Item = Result<V, E>>,
    Lhs: From<V> + MaybeAccumulable<V>,
{
    fn is_terminated(&self) -> bool {
        self.lhs.is_consumed()
    }
}

impl<S, Lhs, V, E> Stream for TryPartiallyAccumulated<S, Lhs>
where
    S: Stream<Item = Result<V, E>>,
    Lhs: From<V> + MaybeAccumulable<V>,
{
    type Item = Result<Lhs, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use AccumulatedState as S;

        let mut proj = self.project();

        let result = loop {
            match proj.lhs {
                S::Uninit => {
                    let first = ready!(proj.stream.as_mut().poll_next(cx));

                    match first {
                        Some(first) => match first {
                            Ok(first) => {
                                *proj.lhs = AccumulatedState::Accumulable(Lhs::from(first));
                            }
                            Err(err) => break Some(Err(err)),
                        },
                        None => break Ok(proj.lhs.consume()).transpose(),
                    }
                }
                S::Accumulable(inner) => {
                    let item = ready!(proj.stream.as_mut().poll_next(cx));

                    match item {
                        Some(item) => match item {
                            Ok(item) => {
                                if inner.maybe_accumulate_from(&item) {
                                    continue;
                                } else {
                                    drop(inner);

                                    break Ok(proj.lhs.reaccumulable(Lhs::from(item))).transpose();
                                }
                            }
                            Err(err) => break Some(Err(err)),
                        },
                        None => {
                            drop(inner);

                            break Ok(proj.lhs.reinit()).transpose();
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
    use futures::stream::{self, TryStreamExt};

    use crate::Accumulable;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Volume(u64);

    impl Accumulable for Volume {
        fn accumulate_from(&mut self, rhs: &Self) {
            *self = Volume(self.0 + rhs.0);
        }
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

        let volumes = stream::iter([] as [Result<VolumeSize100, ()>; 0]);

        let partially_accumulated = volumes
            .try_partially_accumulate::<VolumeSize100>()
            .try_collect::<Vec<_>>()
            .await;

        assert_eq!(partially_accumulated, Ok(vec![]))
    }

    #[tokio::test]
    async fn partially_accumulate_err() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = stream::iter([Ok(VolumeSize100::new(Volume(60))), Err(())]);

        let partially_accumulated = volumes
            .try_partially_accumulate::<VolumeSize100>()
            .try_collect::<Vec<_>>()
            .await;

        assert_eq!(partially_accumulated, Err(()))
    }

    #[tokio::test]
    async fn partially_accumulate_ok() {
        type VolumeSize100 = VolumeSize<100>;

        let volumes = stream::iter([
            Ok::<_, ()>(VolumeSize100::new(Volume(60))),
            Ok(VolumeSize100::new(Volume(30))),
            Ok(VolumeSize100::new(Volume(15))),
            //
            Ok(VolumeSize100::new(Volume(40))),
            Ok(VolumeSize100::new(Volume(70))),
            //
            Ok(VolumeSize100::new(Volume(80))),
            Ok(VolumeSize100::new(Volume(10))),
            Ok(VolumeSize100::new(Volume(5))),
            Ok(VolumeSize100::new(Volume(3))),
            Ok(VolumeSize100::new(Volume(2))),
            //
            Ok(VolumeSize100::new(Volume(1))),
            Ok(VolumeSize100::new(Volume(1))),
        ]);

        let partially_accumulated = volumes
            .try_partially_accumulate::<VolumeSize100>()
            .try_collect::<Vec<_>>()
            .await;

        assert_eq!(
            partially_accumulated,
            Ok(vec![
                VolumeSize100::Large(Volume(105)),
                VolumeSize100::Large(Volume(110)),
                VolumeSize100::Large(Volume(100)),
                VolumeSize100::Small(Volume(2))
            ])
        )
    }
}
