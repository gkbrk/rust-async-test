use std::{sync::OnceLock, task::Waker, time::Instant};

static GLOBALSLEEPTASK: OnceLock<std::sync::mpsc::Sender<(Instant, Box<Waker>)>> = OnceLock::new();

pub fn get_sleep_task() -> &'static std::sync::mpsc::Sender<(Instant, Box<Waker>)> {
    GLOBALSLEEPTASK.get_or_init(|| {
        let (sleep_sender, sleep_receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sleep_task(sleep_receiver);
            panic!("Sleep task terminated!");
        });
        sleep_sender
    })
}

fn sleep_task(recv: std::sync::mpsc::Receiver<(Instant, Box<Waker>)>) {
    let mut sleep_queue: std::collections::BinaryHeap<InstantAndWaker> =
        std::collections::BinaryHeap::new();

    loop {
        while let Ok((instant, waker)) = recv.try_recv() {
            sleep_queue.push(InstantAndWaker(instant, waker));
        }

        let now = Instant::now();
        while let Some(InstantAndWaker(instant, _)) = sleep_queue.peek() {
            if instant <= &now {
                if let Some(InstantAndWaker(_, waker)) = sleep_queue.pop() {
                    waker.wake_by_ref();
                }
            } else {
                break;
            }
        }

        if let Some(InstantAndWaker(instant, _)) = sleep_queue.peek() {
            let timeout = *instant - now;
            match recv.recv_timeout(timeout) {
                Ok((instant, waker)) => {
                    sleep_queue.push(InstantAndWaker(instant, waker));
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        if let Ok((instant, waker)) = recv.recv() {
            sleep_queue.push(InstantAndWaker(instant, waker));
        }
    }
}

struct InstantAndWaker(Instant, Box<Waker>);

impl PartialEq for InstantAndWaker {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for InstantAndWaker {}

impl PartialOrd for InstantAndWaker {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(other.0.cmp(&self.0))
    }
}

impl Ord for InstantAndWaker {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}
