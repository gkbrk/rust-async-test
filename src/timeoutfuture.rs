use std::{future::Future, pin::Pin};

pub struct TimeoutFuture<F, T>
where
    F: Future<Output = T>,
{
    future: F,
    timeout_at: std::time::Instant,
}

pub fn new<F, T>(future: F, timeout: std::time::Duration) -> TimeoutFuture<F, T>
where
    F: Future<Output = T> + Unpin,
{
    TimeoutFuture {
        future,
        timeout_at: std::time::Instant::now() + timeout,
    }
}

pub struct TimeoutError;

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Timeout")
    }
}

impl std::fmt::Debug for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Timeout")
    }
}

impl std::error::Error for TimeoutError {}

impl<F, T> Future for TimeoutFuture<F, T>
where
    F: Future<Output = T> + Unpin,
{
    type Output = std::result::Result<T, TimeoutError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let now = std::time::Instant::now();

        if now >= self.timeout_at {
            return std::task::Poll::Ready(Err(TimeoutError));
        }

        let this = self.get_mut();
        let fut = Pin::new(&mut this.future);
        fut.poll(cx).map(|x| Ok(x))
    }
}
