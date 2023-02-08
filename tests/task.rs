use ex::task::Task;
use std::sync::Weak;

fn spawn_on_thread<F, T>(future: F) -> Task<T, impl std::future::Future<Output = T> + Send + Sync>
where
    for<'a> F: core::future::Future<Output = T> + Send + Sync + 'a,
    T: Send + Sync + 'static,
{
    // Create a channel that holds the task when it is scheduled for running.
    use std::sync::Arc;

    let (sender, receiver) = flume::unbounded::<ex::task::Runnable>();
    let sender = Arc::new(sender);
    let s = Arc::downgrade(&sender);

    // Spawn a thread running the task to completion.
    std::thread::spawn(|| {
        futures_lite::future::block_on(async move {
            // Keep taking the task from the channel and running it until completion.
            for mut runnable in receiver {
                runnable.run().await;
            }
        })
    });

    futures_lite::future::block_on(async move {
        // Wrap the future into one that disconnects the channel on completion.
        let future = async move {
            // When the inner future completes, the sender gets dropped and disconnects the channel.
            let _sender = sender;
            future.await
        };

        // Create a task that is scheduled by sending it into the channel.
        let (runnable, task) = ex::task::spawn(future, move |runnable| {
            let s = Weak::clone(&s);
            Box::pin(async move { s.upgrade().unwrap().send_async(runnable).await.is_ok() })
        })
        .await;

        // Schedule the task by sending it into the channel.
        runnable.schedule().await;

        task
    })
}

#[test]
fn spawn_on_single_thread() {
    // Spawn a future on a dedicated thread.
    let _ = futures_lite::future::block_on(spawn_on_thread(async {
        println!("Hello, world!");
    }));
}
