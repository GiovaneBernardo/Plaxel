use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Worker {
    id: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct JobSystem {
    workers: Vec<Worker>,
    sender: crossbeam_channel::Sender<Job>,
    shutdown: Arc<AtomicBool>,
}

impl JobSystem {
    pub fn new(worker_count: usize) -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded::<Job>();
        let receiver = Arc::new(receiver);
        let shutdown = Arc::new(AtomicBool::new(false));

        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            let shutdown = Arc::clone(&shutdown);

            let thread = thread::Builder::new()
                .name(format!("job-worker-{id}"))
                .spawn(move || {
                    while !shutdown.load(Ordering::Relaxed) {
                        match receiver.recv() {
                            Ok(job) => job(),
                            Err(_) => break,
                        }
                    }
                })
                .expect("failed to spawn job worker");

            workers.push(Worker {
                id,
                thread: Some(thread),
            });
        }

        Self {
            workers,
            sender,
            shutdown,
        }
    }

    pub fn spawn<F>(&self, job: F) -> Result<(), crossbeam_channel::SendError<Job>>
    where
        F: FnOnce() + Send + 'static,
    {
        self.sender.send(Box::new(job))
    }
}

impl Drop for JobSystem {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);

        // Closing the sender wakes workers blocked on recv().
        let (dummy_sender, _dummy_receiver) = crossbeam_channel::unbounded();
        let old_sender = std::mem::replace(&mut self.sender, dummy_sender);
        drop(old_sender);

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
