extern crate alloc;

use alloc::sync::Arc;
use core::any::Any;
use core::future::Future;
use core::marker::PhantomData;
use core::pin::Pin;
use core::task::Waker;

pub mod task {

    use std::{
        sync::{Mutex, Weak},
        task::{Context, Poll},
    };

    use super::*;

    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Error {
        Downcast,
        Poison,
    }

    pub enum FutState<F: Future> {
        Future(Pin<Box<F>>),
        Pending(Pin<Box<F>>, Arc<Waker>),
        Ready(F::Output),
        Done,
    }

    #[derive(Clone)]
    pub struct Runnable {
        state: Arc<dyn Any + Send + Sync>,
        schedule: Arc<dyn Fn(Self) -> BoxFuture<'static, bool> + Send + Sync>,
        waker: Weak<Waker>,
        poll_fn: for<'a, 'b, 'c> fn(&'a mut Self, cx: &'b mut Context<'c>) -> Poll<bool>,
    }

    impl Runnable {
        pub fn poll<
            's,
            F: Future + Send + Sync + 'static,
            R: Fn(Self) -> BoxFuture<'s, bool> + ?Sized,
        >(
            runnable: &mut Self,
            cx: &mut Context<'_>,
        ) -> Poll<bool> {
            let mut locked = runnable
                .state
                .as_ref()
                .downcast_ref::<Mutex<FutState<F>>>()
                .and_then(|s| s.lock().ok());

            let (fut, ref waker) = match locked.as_deref_mut() {
                Some(FutState::Future(ref mut fut)) => (fut, Arc::new(cx.waker().clone())),
                Some(FutState::Pending(ref mut fut, ref waker)) => (fut, waker.clone()),
                _ => return Poll::Ready(false),
            };

            runnable.waker = Arc::downgrade(&waker);
            let mut cx = Context::from_waker(waker);

            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(o) => {
                    locked.as_deref_mut().map_or((), |locked| {
                        *locked = FutState::Ready(o);
                        waker.wake_by_ref();
                        drop(locked);
                    });

                    Poll::Ready(true)
                }
                Poll::Pending => {
                    drop(locked);
                    // re-schedule it
                    match Box::pin(runnable.schedule()).as_mut().poll(&mut cx) {
                        Poll::Ready(b) => Poll::Ready(b),
                        Poll::Pending => Poll::Pending,
                    }
                }
            }
        }

        pub async fn run(&mut self) -> bool {
            core::future::poll_fn(|cx| (self.poll_fn)(self, cx)).await
        }

        pub async fn schedule(&self) -> bool {
            (self.schedule)(Runnable {
                state: self.state.clone(),
                schedule: self.schedule.clone(),
                waker: self.waker.clone(),
                poll_fn: self.poll_fn.clone(),
            })
            .await
        }
    }

    impl Drop for Runnable {
        fn drop(&mut self) {
            self.waker.upgrade().map_or((), |w| w.wake_by_ref())
        }
    }

    pub struct Task<T, F: Future<Output = T> + Send + Sync> {
        raw: Arc<Mutex<FutState<F>>>,
        _data: PhantomData<(T, F)>,
    }

    impl<T, F: Future<Output = T> + Send + Sync> Future for Task<T, F> {
        type Output = Result<F::Output, Error>;

        fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<Self::Output> {
            let mut self_ = self.raw.lock().map_err(|_| Error::Poison)?;

            *self_ = match core::mem::replace(&mut *self_, FutState::Done) {
                FutState::Ready(o) => return Poll::Ready(Ok(o)),
                FutState::Future(fut) => FutState::Pending(fut, Arc::new(cx.waker().clone())),
                FutState::Pending(fut, w) if !w.will_wake(cx.waker()) => {
                    FutState::Pending(fut, Arc::new(cx.waker().clone()))
                }
                state => state,
            };

            Poll::Pending
        }
    }

    pub async fn spawn_unchecked<
        T: Send + Sync,
        F: Future<Output = T> + Send + Sync + 'static,
        R: Fn(Runnable) -> BoxFuture<'static, bool> + Send + Sync + 'static,
    >(
        future: F,
        schedule: R,
    ) -> (Runnable, Task<T, F>) {
        let state = Arc::new(Mutex::new(FutState::Future(Box::pin(future))));

        let runnable = Runnable {
            state: state.clone(),
            schedule: Arc::new(schedule),
            waker: Weak::new(),
            poll_fn: Runnable::poll::<F, R>,
        };

        let task = Task {
            raw: state,
            _data: PhantomData,
        };

        runnable.schedule().await;

        (runnable, task)
    }

    pub async fn spawn<
        T: Send + Sync + 'static,
        F: Future<Output = T> + Send + Sync + 'static,
        R: Fn(Runnable) -> BoxFuture<'static, bool> + Send + Sync + 'static,
    >(
        future: F,
        schedule: R,
    ) -> (Runnable, Task<T, F>) {
        spawn_unchecked(future, schedule).await
    }
}

pub mod executor {

    use super::*;
    use crate::task::spawn_unchecked;
    use crate::task::BoxFuture;
    use crate::task::Runnable;
    use crate::task::Task;

    struct State {
        sender: flume::Sender<Runnable>,
        receiver: flume::Receiver<Runnable>,
    }

    impl State {
        pub fn new() -> Self {
            let (sender, receiver) = flume::unbounded::<Runnable>();
            Self { sender, receiver }
        }
    }

    #[derive(Clone)]
    pub struct Executor<'e> {
        state: Arc<State>,
        _data: PhantomData<&'e ()>,
    }

    impl<'e> Default for Executor<'e> {
        fn default() -> Self {
            Self {
                state: Arc::new(State::new()),
                _data: PhantomData,
            }
        }
    }

    impl<'e> Executor<'e> {
        // complete
        pub async fn spawn<T: Send + Sync, F: Future<Output = T> + Send + Sync + 'static>(
            &self,
            future: F,
        ) -> Task<T, impl Future<Output = T> + Send + Sync> {
            let future = async move { future.await };

            // Create the task and register it in the set of active tasks.
            let (runnable, task) = spawn_unchecked(future, self.schedule()).await;

            runnable.schedule().await;

            task
        }

        pub async fn run<T>(&self, future: impl Future<Output = T>) -> T {
            let state = self.state.clone();

            let run_forever = async {
                loop {
                    for _ in 0..200 {
                        if let Ok(mut runnable) = state.receiver.recv_async().await {
                            runnable.run().await;
                        }
                    }

                    futures_lite::future::yield_now().await;
                }
            };

            futures_lite::FutureExt::or(future, run_forever).await
        }

        pub async fn tick(&self) -> bool {
            match self.state.receiver.recv_async().await {
                Ok(mut runnable) => runnable.run().await,
                Err(_) => false,
            }
        }

        fn schedule(&self) -> impl Fn(Runnable) -> BoxFuture<'static, bool> {
            let state = self.state.clone();

            move |runnable| {
                let state = state.clone();
                Box::pin(async move { state.sender.send_async(runnable).await.is_ok() })
            }
        }
    }
}
