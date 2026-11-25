//! `tauri::async_runtime` as version 1's discovery code uses it: `spawn` onto one shared runtime, and
//! tokio's `JoinHandle` (which has the `abort` and the `Future` that code relies on).
pub mod async_runtime {
    use std::future::Future;
    use std::sync::OnceLock;

    pub use tokio::task::JoinHandle;

    fn runtime() -> &'static tokio::runtime::Runtime {
        static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("fp-net")
                .enable_all()
                .build()
                .expect("tokio runtime")
        })
    }

    pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        runtime().spawn(task)
    }
}
