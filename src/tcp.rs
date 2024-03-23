use std::{
    future::Future,
    os::fd::{AsRawFd, BorrowedFd},
    sync::Arc,
};

use crate::{fn_future, get_executor, DSSResult, PollType, TaskExecutor};

#[derive(Clone)]
pub struct TcpStream {
    _stream: Arc<std::net::TcpStream>,
    fd: i32,
    executor: TaskExecutor,
}

impl TcpStream {
    pub fn new_with_global_executor(stream: std::net::TcpStream) -> Self {
        Self::new(stream, get_executor().lock().unwrap().clone())
    }

    fn new(stream: std::net::TcpStream, executor: TaskExecutor) -> Self {
        let fd = stream.as_raw_fd();
        Self {
            _stream: Arc::new(stream),
            fd,
            executor,
        }
    }

    pub fn write<'a>(&'a self, data: &'a [u8]) -> impl Future<Output = DSSResult<usize>> + 'a {
        std::future::poll_fn(move |cx| {
            let fd = unsafe { BorrowedFd::borrow_raw(self.fd) };
            match nix::unistd::write(fd, data) {
                Ok(n) => std::task::Poll::Ready(Ok(n)),
                Err(nix::errno::Errno::EAGAIN) => {
                    self.executor
                        .poll_sender
                        .send((self.fd, PollType::Write, cx.waker().clone()))
                        .unwrap();
                    std::task::Poll::Pending
                }
                Err(e) => std::task::Poll::Ready(Err(e.into())),
            }
        })
    }

    pub fn read<'a>(&'a self, data: &'a mut [u8]) -> impl Future<Output = DSSResult<usize>> + 'a {
        std::future::poll_fn(move |cx| match nix::unistd::read(self.fd, data) {
            Ok(n) => std::task::Poll::Ready(Ok(n)),
            Err(nix::errno::Errno::EAGAIN) => {
                self.executor
                    .poll_sender
                    .send((self.fd, PollType::Read, cx.waker().clone()))
                    .unwrap();
                std::task::Poll::Pending
            }
            Err(e) => std::task::Poll::Ready(Err(e.into())),
        })
    }
}

pub fn tcpaccept(
    listener: &std::net::TcpListener,
) -> impl Future<Output = DSSResult<std::net::TcpStream>> + '_ {
    std::future::poll_fn(move |cx| {
        let stream = listener.accept();
        match stream {
            Ok((stream, _)) => {
                stream.set_nonblocking(true)?;
                stream.set_nodelay(true)?;
                std::task::Poll::Ready(Ok(stream))
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                get_executor()
                    .lock()
                    .unwrap()
                    .poll_sender
                    .send((listener.as_raw_fd(), PollType::Read, cx.waker().clone()))
                    .unwrap();
                std::task::Poll::Pending
            }
            Err(e) => std::task::Poll::Ready(Err(e.into())),
        }
    })
}

pub async fn tcpdial(addr: &str, port: u16) -> DSSResult<TcpStream> {
    let addr = format!("{}:{}", addr, port);

    let stream = fn_future(move || -> DSSResult<std::net::TcpStream> {
        let stream = std::net::TcpStream::connect(addr)?;
        stream.set_nonblocking(true)?;
        Ok(stream)
    })
    .await?;

    Ok(TcpStream::new_with_global_executor(stream))
}
