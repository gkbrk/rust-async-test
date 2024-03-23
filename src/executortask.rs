use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

pub struct Task {
    pub(crate) future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>>,
    pub(crate) task_sender: crossbeam::channel::Sender<Arc<Task>>,
}

impl std::task::Wake for Task {
    fn wake(self: Arc<Self>) {
        let sender = self.task_sender.clone();
        sender.send(self).unwrap();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.task_sender.send(self.clone()).unwrap();
    }
}
