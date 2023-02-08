use std::{future::Future, pin::Pin, sync::Arc};

use criterion::async_executor::FuturesExecutor;
use criterion::{criterion_group, criterion_main, Criterion};
use ex::executor::Executor;

const TASKS: usize = 300;
const STEPS: usize = 300;
const LIGHT_TASKS: usize = 25_000;

fn create(c: &mut Criterion) {
    c.bench_function("executor::create", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let ex = Executor::default();
            let task = ex.spawn(async {}).await;
            assert_eq!(ex.run(task).await, Ok(()));
        })
    });
}

fn create_smol(c: &mut Criterion) {
    c.bench_function("executor::create_smol", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let ex = async_executor::Executor::default();
            let task = ex.spawn(async {});
            let _ = futures_lite::future::block_on(ex.run(task));
        })
    });
}

fn spawn_one(c: &mut Criterion) {
    c.bench_function("executor::spawn_one", |b| {
        let ex: Arc<Executor<'_>> = Arc::new(Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| ex.spawn(async {}));
    });
}

fn spawn_one_smol(c: &mut Criterion) {
    c.bench_function("executor::spawn_one_smol", |b| {
        let ex = Arc::new(async_executor::Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| ex.spawn(async {}));
    });
}

fn spawn_many(c: &mut Criterion) {
    c.bench_function("executor::spawn_many_local", |b| {
        let ex: Arc<Executor<'_>> = Arc::new(Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let mut tasks = Vec::new();
            for _ in 0..LIGHT_TASKS {
                tasks.push(ex.spawn(async {}).await);
            }
            for task in tasks {
                assert_eq!(task.await, Ok(()));
            }
        });
    });
}

fn spawn_many_smol(c: &mut Criterion) {
    c.bench_function("executor::spawn_many_local_smol", |b| {
        let ex = Arc::new(async_executor::Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let mut tasks = Vec::new();
            for _ in 0..LIGHT_TASKS {
                tasks.push(ex.spawn(async {}));
            }
            for task in tasks {
                assert_eq!(task.await, ());
            }
        });
    });
}

fn spawn_recursively(c: &mut Criterion) {
    pub fn async_fibo(
        ex: Arc<Executor<'static>>,
        n: u64,
    ) -> Pin<std::boxed::Box<dyn Future<Output = u64> + Send + Sync + 'static>> {
        Box::pin(async move {
            match n {
                0 => 0,
                1 => 1,
                _ => {
                    ex.clone()
                        .spawn(async_fibo(ex.clone(), n - 1))
                        .await
                        .await
                        .unwrap()
                        + ex.clone().spawn(async_fibo(ex, n - 2)).await.await.unwrap()
                }
            }
        })
    }

    c.bench_function("executor::spawn_recursively", |b| {
        let ex: Arc<Executor<'_>> = Arc::new(Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let ex_ = Arc::clone(&ex);
            let mut tasks = Vec::new();
            for _ in 0..TASKS {
                let ex__ = Arc::clone(&ex_);
                tasks.push(ex_.spawn(async_fibo(ex__, 10)).await);
            }

            for task in tasks {
                assert_eq!(task.await, Ok(55));
            }
        });
    });
}

fn spawn_recursively_smol(c: &mut Criterion) {
    pub fn async_fibo(
        ex: Arc<async_executor::Executor<'static>>,
        n: u64,
    ) -> Pin<std::boxed::Box<dyn Future<Output = u64> + Send + Sync + 'static>> {
        Box::pin(async move {
            match n {
                0 => 0,
                1 => 1,
                _ => {
                    ex.clone().spawn(async_fibo(ex.clone(), n - 1)).await
                        + ex.clone().spawn(async_fibo(ex, n - 2)).await
                }
            }
        })
    }

    c.bench_function("executor::spawn_recursively_smol", |b| {
        let ex = Arc::new(async_executor::Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let ex_ = Arc::clone(&ex);
            let mut tasks = Vec::new();
            for _ in 0..TASKS {
                let ex__ = Arc::clone(&ex_);
                tasks.push(ex_.spawn(async_fibo(ex__, 10)));
            }

            for task in tasks {
                assert_eq!(task.await, 55);
            }
        });
    });
}

fn yield_now(c: &mut Criterion) {
    c.bench_function("executor::yield_now", |b| {
        let ex: Arc<Executor<'_>> = Arc::new(Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let mut tasks = Vec::new();
            for _ in 0..TASKS {
                tasks.push(
                    ex.spawn(async move {
                        for _ in 0..STEPS {
                            futures_lite::future::yield_now().await;
                        }
                    })
                    .await,
                );
            }
            for task in tasks {
                assert_eq!(task.await, Ok(()));
            }
        });
    });
}

fn yield_now_smol(c: &mut Criterion) {
    c.bench_function("executor::yield_now_smol", |b| {
        let ex = Arc::new(async_executor::Executor::default());

        // Spawn a thread running the executor forever.
        for _ in 0..num_cpus::get() {
            std::thread::spawn({
                let _ex = Arc::clone(&ex);
                move || futures_lite::future::block_on(_ex.run(core::future::pending::<()>()))
            });
        }

        b.to_async(FuturesExecutor).iter(|| async {
            let mut tasks = Vec::new();
            for _ in 0..TASKS {
                tasks.push(ex.spawn(async move {
                    for _ in 0..STEPS {
                        futures_lite::future::yield_now().await;
                    }
                }));
            }
            for task in tasks {
                assert_eq!(task.await, ());
            }
        });
    });
}

criterion_group!(
    benches,
    create,
    create_smol,
    spawn_one,
    spawn_one_smol,
    spawn_many,
    spawn_many_smol,
    spawn_recursively,
    spawn_recursively_smol,
    yield_now,
    yield_now_smol,
);

criterion_main!(benches);
