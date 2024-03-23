use crate::PollType;
use nix::sys::epoll::{Epoll, EpollCreateFlags};
use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Waker;

type WakerHashMap = HashMap<(i32, PollType), Waker>;

pub(crate) fn epoll_task() -> crossbeam::channel::Sender<(i32, PollType, std::task::Waker)> {
    let (poll_sender, poll_receiver) =
        crossbeam::channel::unbounded::<(i32, PollType, std::task::Waker)>();

    let waker_hashmap: Arc<Mutex<WakerHashMap>> = Arc::new(Mutex::new(HashMap::new()));

    let epoll = Arc::new(Epoll::new(EpollCreateFlags::empty()).unwrap());

    {
        let waker_hashmap = waker_hashmap.clone();
        let epoll = epoll.clone();

        std::thread::spawn(move || {
            channel_to_epoll_task(poll_receiver, waker_hashmap, epoll);
        });
    }

    std::thread::spawn(move || {
        epoll_to_waker_task(waker_hashmap, epoll);
    });

    poll_sender
}

fn channel_to_epoll_task(
    poll_receiver: crossbeam::channel::Receiver<(i32, PollType, Waker)>,
    waker_hashmap: Arc<Mutex<WakerHashMap>>,
    epoll: Arc<Epoll>,
) {
    loop {
        let (fd, polltype, waker) = poll_receiver.recv().unwrap();

        let flags = match polltype {
            PollType::Read => {
                nix::sys::epoll::EpollFlags::EPOLLIN | nix::sys::epoll::EpollFlags::EPOLLONESHOT
            }
            PollType::Write => {
                nix::sys::epoll::EpollFlags::EPOLLOUT | nix::sys::epoll::EpollFlags::EPOLLONESHOT
            }
        };

        // Add to hashmap
        let old = waker_hashmap.lock().unwrap().insert((fd, polltype), waker);

        if old.is_some() {
            // TODO: Perhaps this should be a log message
            panic!("waker already exists");
        }

        let rawfd = fd;
        let fd = unsafe { BorrowedFd::borrow_raw(fd) };

        let mut ev = nix::sys::epoll::EpollEvent::new(flags, rawfd as u64);

        match epoll.modify(fd, &mut ev) {
            Ok(_) => {}
            Err(nix::errno::Errno::ENOENT) => {
                // Add
                epoll.add(fd, ev).unwrap();
            }
            Err(e) => {
                panic!("epoll modify error: {:?}", e);
            }
        }
    }
}

fn epoll_to_waker_task(
    waker_hashmap: Arc<Mutex<WakerHashMap>>,
    epoll: Arc<Epoll>,
) -> crossbeam::channel::Sender<(i32, PollType)> {
    let mut events = [nix::sys::epoll::EpollEvent::empty(); 64];

    loop {
        let n = epoll
            .wait(&mut events, nix::sys::epoll::EpollTimeout::NONE)
            .unwrap();

        let mut waker_hashmap = waker_hashmap.lock().unwrap();

        for event in events.iter().take(n) {
            let fd = event.data() as i32;

            let event = event.events();

            if event.contains(nix::sys::epoll::EpollFlags::EPOLLIN) {
                if let Some(waker) = waker_hashmap.remove(&(fd, PollType::Read)) {
                    waker.wake();
                }
            }

            if event.contains(nix::sys::epoll::EpollFlags::EPOLLOUT) {
                if let Some(waker) = waker_hashmap.remove(&(fd, PollType::Write)) {
                    waker.wake();
                }
            }
        }
    }
}
