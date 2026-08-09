use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
};

struct Worker {
    id: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

struct QueuedJob {
    name: String,
    priority: Arc<AtomicU32>,
    sequence: u64,
    job: Job,
}

#[derive(Clone, Debug)]
pub struct JobPriorityHandle {
    priority: Arc<AtomicU32>,
}

impl JobPriorityHandle {
    pub fn set(&self, priority: u32) {
        self.priority.store(priority, Ordering::Relaxed);
    }
}

pub struct JobSystem {
    workers: Vec<Worker>,
    queue: Arc<(Mutex<Vec<QueuedJob>>, Condvar)>,
    shutdown: Arc<AtomicBool>,
    next_sequence: AtomicU64,
    queued: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
}

impl JobSystem {
    pub fn new(worker_count: usize) -> Self {
        let queue = Arc::new((Mutex::new(Vec::<QueuedJob>::new()), Condvar::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::with_capacity(worker_count);

        for id in 0..worker_count {
            let queue = Arc::clone(&queue);
            let shutdown = Arc::clone(&shutdown);
            let queued = Arc::clone(&queued);
            let running = Arc::clone(&running);
            let completed = Arc::clone(&completed);

            let thread = thread::Builder::new()
                .name(format!("job-worker-{id}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let (jobs, wake) = &*queue;
                            let mut jobs = jobs.lock().unwrap();
                            while jobs.is_empty() && !shutdown.load(Ordering::Relaxed) {
                                jobs = wake.wait(jobs).unwrap();
                            }
                            if shutdown.load(Ordering::Relaxed) {
                                break;
                            }

                            let best = jobs
                                .iter()
                                .enumerate()
                                .max_by(|(_, a), (_, b)| {
                                    a.priority
                                        .load(Ordering::Relaxed)
                                        .cmp(&b.priority.load(Ordering::Relaxed))
                                        // Earlier submission wins equal priority.
                                        .then_with(|| b.sequence.cmp(&a.sequence))
                                })
                                .map(|(index, _)| index)
                                .unwrap();
                            jobs.swap_remove(best)
                        };

                        queued.fetch_sub(1, Ordering::Relaxed);
                        running.fetch_add(1, Ordering::Relaxed);
                        {
                            crate::profile_scope!("job.execute");
                            crate::profile_dynamic_scope!("job.named", job.name.clone());
                            (job.job)();
                        }
                        running.fetch_sub(1, Ordering::Relaxed);
                        completed.fetch_add(1, Ordering::Relaxed);
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
            queue,
            shutdown,
            next_sequence: AtomicU64::new(0),
            queued,
            running,
            completed,
        }
    }

    pub fn spawn<F>(&self, job: F) -> Result<(), crossbeam_channel::SendError<Job>>
    where
        F: FnOnce() + Send + 'static,
    {
        self.spawn_named("job", job)
    }

    pub fn spawn_named<F>(
        &self,
        name: impl Into<String>,
        job: F,
    ) -> Result<(), crossbeam_channel::SendError<Job>>
    where
        F: FnOnce() + Send + 'static,
    {
        self.spawn_prioritized_named(name, 0, job).map(|_| ())
    }

    pub fn spawn_prioritized<F>(
        &self,
        priority: u32,
        job: F,
    ) -> Result<JobPriorityHandle, crossbeam_channel::SendError<Job>>
    where
        F: FnOnce() + Send + 'static,
    {
        self.spawn_prioritized_named("job", priority, job)
    }

    pub fn spawn_prioritized_named<F>(
        &self,
        name: impl Into<String>,
        priority: u32,
        job: F,
    ) -> Result<JobPriorityHandle, crossbeam_channel::SendError<Job>>
    where
        F: FnOnce() + Send + 'static,
    {
        let job: Job = Box::new(job);
        if self.shutdown.load(Ordering::Relaxed) {
            return Err(crossbeam_channel::SendError(job));
        }

        let priority = Arc::new(AtomicU32::new(priority));
        let handle = JobPriorityHandle {
            priority: Arc::clone(&priority),
        };
        let queued_job = QueuedJob {
            name: name.into(),
            priority,
            sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
            job,
        };

        self.queued.fetch_add(1, Ordering::Relaxed);
        crate::profile_counter!("jobs.queued", self.queued.load(Ordering::Relaxed) as f64);
        crate::profile_counter!("jobs.running", self.running.load(Ordering::Relaxed) as f64);
        crate::profile_counter!(
            "jobs.completed",
            self.completed.load(Ordering::Relaxed) as f64
        );

        let (queue, wake) = &*self.queue;
        queue.lock().unwrap().push(queued_job);
        wake.notify_one();
        Ok(handle)
    }
}

impl Drop for JobSystem {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.queue.1.notify_all();

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
