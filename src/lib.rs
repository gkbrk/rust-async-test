mod asyncwrite;
mod epoll_task;
mod executortask;
mod future_ext;
mod sleep_task;
pub mod tcp;
mod timeoutfuture;

use executortask::Task;

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Waker;

use future_ext::FutureMap;

type GlobalRandomExecutor = OnceLock<Mutex<TaskExecutor>>;
static EXECUTOR: GlobalRandomExecutor = OnceLock::new();

pub type DSSResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
enum PollType {
    Read,
    Write,
}

#[derive(Clone)]
pub struct TaskExecutor {
    task_sender: Option<crossbeam::channel::Sender<Arc<Task>>>,
    task_receiver: crossbeam::channel::Receiver<Arc<Task>>,
    poll_sender: crossbeam::channel::Sender<(i32, PollType, std::task::Waker)>,
}

fn get_executor() -> &'static Mutex<TaskExecutor> {
    EXECUTOR.get_or_init(|| {
        let (task_sender, task_receiver) = crossbeam::channel::unbounded();
        let executor = TaskExecutor {
            task_sender: Some(task_sender),
            task_receiver,
            poll_sender: epoll_task::epoll_task(),
        };
        Mutex::new(executor)
    })
}

pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let sender = {
        let executor = get_executor().lock().unwrap();
        executor.task_sender.clone()
    };

    let task = Arc::new(Task {
        future: Mutex::new(Some(Box::pin(future))),
        task_sender: sender.clone().unwrap(),
    });
    sender.clone().unwrap().send(task).unwrap();
}

pub fn run_main<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (result_sender, result_receiver) = std::sync::mpsc::channel();

    {
        // Create an executor to make sure it is initialized.
        get_executor();
    }

    spawn(async move {
        let res = future.await;

        // Take the global task sender, so the executor can be shut down.
        {
            let mut executor = get_executor().lock().unwrap();
            executor.task_sender.take();
        }

        result_sender.send(res).unwrap();
    });

    // Runners
    let mut runners = Vec::new();

    let parallelism = std::thread::available_parallelism()
        .unwrap_or(NonZeroUsize::new(8).unwrap())
        .get();

    for _ in 0..parallelism {
        let t = std::thread::spawn(run_forever);
        runners.push(t);
    }

    for r in runners {
        r.join().unwrap();
    }

    let res = result_receiver.try_recv();
    res.unwrap()
}

fn run_forever() {
    let queue = {
        let executor = get_executor().lock().unwrap();
        executor.task_receiver.clone()
    };

    loop {
        let task = match queue.recv() {
            Ok(task) => task,
            Err(_) => return,
        };

        let mut future_slot = task.future.lock().unwrap();

        if let Some(mut future) = future_slot.take() {
            let waker = std::task::Waker::from(task.clone());
            let context = &mut std::task::Context::from_waker(&waker);

            if future.as_mut().poll(context).is_pending() {
                // Not done, put it back.
                *future_slot = Some(future);
            }
        }
    }
}

pub fn sleep_ms(duration_ms: usize) -> impl Future<Output = ()> {
    let target = std::time::Instant::now() + std::time::Duration::from_millis(duration_ms as u64);

    std::future::poll_fn(move |cx| {
        let now = std::time::Instant::now();

        if now >= target {
            std::task::Poll::Ready(())
        } else {
            // Register a sleep waker
            sleep_task::get_sleep_task()
                .send((target, Box::new(cx.waker().clone())))
                .unwrap();
            std::task::Poll::Pending
        }
    })
}

struct SelectTwoFuture<F1, F2, T>
where
    F1: Future<Output = T>,
    F2: Future<Output = T>,
{
    future1: F1,
    future2: F2,
}

impl<F1, F2, T> Future for SelectTwoFuture<F1, F2, T>
where
    F1: Future<Output = T> + Unpin,
    F2: Future<Output = T> + Unpin,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context) -> std::task::Poll<T> {
        let this = self.get_mut();

        let poll1 = Pin::new(&mut this.future1).poll(cx);
        let poll2 = Pin::new(&mut this.future2).poll(cx);

        match (poll1, poll2) {
            (std::task::Poll::Ready(val), _) => std::task::Poll::Ready(val),
            (_, std::task::Poll::Ready(val)) => std::task::Poll::Ready(val),
            (std::task::Poll::Pending, std::task::Poll::Pending) => std::task::Poll::Pending,
        }
    }
}

pub fn select_two<F1, F2, T>(future1: F1, future2: F2) -> impl Future<Output = T>
where
    F1: Future<Output = T>,
    F2: Future<Output = T>,
{
    SelectTwoFuture {
        future1: Box::pin(future1),
        future2: Box::pin(future2),
    }
}

fn fn_future<T>(f: impl FnOnce() -> T + Send + Sync + 'static) -> impl Future<Output = T>
where
    T: Send + 'static,
{
    let result = Arc::new(crossbeam::atomic::AtomicCell::new(None));
    let waker: Arc<crossbeam::atomic::AtomicCell<Option<Waker>>> =
        Arc::new(crossbeam::atomic::AtomicCell::new(None));

    let f = Arc::new(Mutex::new(Some(f)));

    let pollfn = {
        let result = result.clone();
        let waker = waker.clone();
        std::future::poll_fn(move |cx| {
            waker.store(Some(cx.waker().clone()));

            match result.take() {
                Some(res) => std::task::Poll::Ready(res),
                None => std::task::Poll::Pending,
            }
        })
    };

    {
        let result = result.clone();
        let waker = waker.clone();
        std::thread::spawn(move || {
            let f = f.lock().unwrap().take().unwrap();
            let res = f();
            result.store(Some(res));

            loop {
                match waker.take() {
                    Some(waker) => {
                        waker.wake();
                        break;
                    }
                    None => std::thread::yield_now(),
                }
            }
        });
    }

    pollfn
}

struct Join2Futures<F1, F2, T1, T2>
where
    F1: Future<Output = T1>,
    F2: Future<Output = T2>,
{
    future1: F1,
    future2: F2,
    future1_result: Option<T1>,
    future2_result: Option<T2>,
}

impl<F1, F2, T1, T2> Future for Join2Futures<F1, F2, T1, T2>
where
    F1: Future<Output = T1> + Unpin,
    F2: Future<Output = T2> + Unpin,
    T1: Unpin,
    T2: Unpin,
{
    type Output = (T1, T2);

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context) -> std::task::Poll<(T1, T2)> {
        let this = self.get_mut();

        if this.future1_result.is_none() {
            match Pin::new(&mut this.future1).poll(cx) {
                std::task::Poll::Ready(val) => this.future1_result = Some(val),
                std::task::Poll::Pending => {}
            }
        }

        if this.future2_result.is_none() {
            match Pin::new(&mut this.future2).poll(cx) {
                std::task::Poll::Ready(val) => this.future2_result = Some(val),
                std::task::Poll::Pending => {}
            }
        }

        if this.future1_result.is_none() || this.future2_result.is_none() {
            return std::task::Poll::Pending;
        }

        std::task::Poll::Ready((
            this.future1_result.take().unwrap(),
            this.future2_result.take().unwrap(),
        ))
    }
}

pub fn join2futures<F1, F2, T1, T2>(future1: F1, future2: F2) -> impl Future<Output = (T1, T2)>
where
    F1: Future<Output = T1>,
    F2: Future<Output = T2>,
    T1: Unpin,
    T2: Unpin,
{
    Join2Futures {
        future1: Box::pin(future1),
        future2: Box::pin(future2),
        future1_result: None,
        future2_result: None,
    }
}

pub fn join3futures<F1, F2, F3, T1, T2, T3>(
    future1: F1,
    future2: F2,
    future3: F3,
) -> impl Future<Output = (T1, T2, T3)>
where
    F1: Future<Output = T1>,
    F2: Future<Output = T2>,
    F3: Future<Output = T3>,
    T1: Unpin,
    T2: Unpin,
    T3: Unpin,
{
    let join2 = join2futures(future1, future2);
    let join3 = join2futures(join2, future3);
    join3.map(|((a, b), c)| (a, b, c))
}

pub fn future_with_timeout<F, T>(
    future: F,
    timeout: std::time::Duration,
) -> impl Future<Output = std::result::Result<T, timeoutfuture::TimeoutError>>
where
    F: Future<Output = T> + Unpin,
{
    timeoutfuture::new(future, timeout)
}

struct YieldFuture(bool);

impl Future for YieldFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context) -> std::task::Poll<()> {
        if self.0 {
            std::task::Poll::Ready(())
        } else {
            self.get_mut().0 = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }
}

pub fn yield_now() -> impl Future<Output = ()> {
    YieldFuture(false)
}
