extern crate alloc;

use alloc::sync::Arc;
use ex::task::Task;
use ex::{executor::Executor, task::Error};
use futures_lite::{future, prelude::*};

/// Task priority.
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
enum Priority {
    High = 0,
    Medium = 1,
    Low = 2,
}

/// An executor with task priorities.
///
/// Tasks with lower priorities only get polled when there are no tasks with higher priorities.
#[derive(Default)]
struct PriorityExecutor<'a> {
    ex: [Executor<'a>; 3],
}

impl<'p> PriorityExecutor<'p> {
    /// Spawns a task with the given priority.
    async fn spawn<T: Send + Sync, F>(
        &self,
        priority: Priority,
        future: F,
    ) -> Task<T, impl Future<Output = T> + Send + Sync>
    where
        for<'a> F: Future<Output = T> + Send + Sync + 'a,
    {
        self.ex[priority as usize].spawn(future).await
    }

    /// Runs the executor forever.
    async fn run(&self) {
        loop {
            for _ in 0..200 {
                let t0 = self.ex[0].tick();
                let t1 = self.ex[1].tick();
                let t2 = self.ex[2].tick();

                // Wait until one of the ticks completes, trying them in order from highest
                // priority to lowest priority.
                let _ = t0.or(t1).or(t2).await;
            }

            // Yield every now and then.
            future::yield_now().await;
        }
    }
}

pub struct ThreadGuard(Arc<PriorityExecutor<'static>>);

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        std::thread::spawn({
            let _guard = Self(Arc::clone(&self.0));
            move || future::block_on(_guard.0.run())
        });
    }
}

#[test]
fn run_executor() {
    let ex: Arc<PriorityExecutor<'_>> = Arc::new(PriorityExecutor::default());

    // Spawn a thread running the executor forever.
    std::thread::spawn({
        let _guard = ThreadGuard(Arc::clone(&ex));
        move || future::block_on(_guard.0.run())
    });

    futures_lite::future::block_on(async {
        let mut tasks = Vec::new();

        for i in 0..20 {
            // Choose a random priority.
            let choice = [Priority::High, Priority::Medium, Priority::Low];
            let priority = choice[fastrand::usize(..choice.len())];

            // Spawn a task with this priority.
            tasks.push(
                ex.spawn(priority, async move {
                    println!("{:?}", priority);
                    future::yield_now().await;
                    println!("{:?}", priority);

                    if i == 1 {
                        panic!("Intended panic, to check panic-safety");
                    }
                })
                .await,
            );
        }

        for (index, task) in tasks.into_iter().enumerate() {
            assert_eq!(
                task.await,
                if index == 1 {
                    Err(Error::Poison)
                } else {
                    Ok(())
                }
            );
        }
    });
}
