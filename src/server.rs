use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::net::{TcpListener, ToSocketAddrs};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use anyhow::Result;
use io_uring::cqueue::Entry as Cqe;
use io_uring::opcode::AcceptMulti;
use io_uring::types::Fd;
use io_uring::IoUring;

use crate::buffer::BufferPool;
use crate::client::Client;
use crate::common::{Id, Route};
use crate::utils::Errno;

const URING_BUFFER_SIZE: u32 = 1024;
const BUFFERS_COUNT: u16 = 8192;
const BUFFER_SIZE: u32 = 32_768;

static VTABLE_STUB: RawWakerVTable = RawWakerVTable::new(
    |ptr| RawWaker::new(ptr, &VTABLE_STUB),
    |_| {},
    |_| {},
    |_| {},
);

pub struct Server {
    listener: TcpListener,
    ring: Rc<RefCell<IoUring>>,
    buffer_pool: BufferPool,
    clients: HashMap<Id, Task>,
    client_id_counter: u32,
}

impl Server {
    pub fn bind(bind_address: impl ToSocketAddrs) -> Self {
        let listener = TcpListener::bind(bind_address).expect("bind");

        let ring = IoUring::builder()
            .build(URING_BUFFER_SIZE)
            .expect("build io_uring");

        let buffer_pool = BufferPool::new(BUFFERS_COUNT, BUFFER_SIZE);
        let iovecs = buffer_pool.iovecs();
        unsafe { ring.submitter().register_buffers(&iovecs) }.expect("register buffers");

        Self {
            listener,
            ring: Rc::new(RefCell::new(ring)),
            buffer_pool,
            clients: HashMap::new(),
            client_id_counter: 0,
        }
    }

    pub fn run(mut self) {
        self.start_accepting();

        loop {
            let Some(cqe) = self.wait_event() else {
                continue;
            };

            match cqe.user_data().into() {
                Route::Accept => self.handle_accept(cqe),
                Route::Client(id) => self.handle_client(cqe, id),
            }
        }
    }

    fn start_accepting(&mut self) {
        let sqe = AcceptMulti::new(Fd(self.listener.as_raw_fd()))
            .build()
            .user_data(Route::Accept.into());

        let mut ring = self.ring.borrow_mut();
        unsafe { ring.submission().push(&sqe) }.expect("push multi-accept");
        ring.submit().expect("submit accept");
    }

    fn wait_event(&self) -> Option<Cqe> {
        let mut ring = self.ring.borrow_mut();
        unsafe { ring.submitter().enter(0, 1, 0, None as Option<&()>) }.expect("wait for event");
        let cqe = ring.completion().next()?;
        Some(cqe)
    }

    fn handle_accept(&mut self, cqe: Cqe) {
        if !io_uring::cqueue::more(cqe.flags()) {
            eprintln!("The acceptor will not accept anymore");
        }

        if cqe.result() < 0 {
            eprintln!("Accept error: {}", Errno(-cqe.result()));
        } else {
            let raw_fd = RawFd::from(cqe.result());
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

            if let Some(buffer) = self.buffer_pool.acquire() {
                let id = self.client_id_counter;
                self.client_id_counter += 1;
                let cqe = Rc::new(RefCell::new(None));

                let mut client =
                    Client::new(id, fd, buffer, Rc::clone(&self.ring), Rc::clone(&cqe));

                let fut = Box::pin(async move { client.handle().await });
                let mut task = Task { fut, cqe };

                match task.poll() {
                    Poll::Pending => {
                        self.clients.insert(id, task);
                    }
                    Poll::Ready(Ok(())) => (),
                    Poll::Ready(Err(err)) => eprintln!("Client #{id} failed: {err:#}"),
                }
            } else {
                eprintln!("No free buffers, disconnecting client");
            }
        }
    }

    fn handle_client(&mut self, cqe: Cqe, id: Id) {
        if let Some(task) = self.clients.get_mut(&id) {
            *task.cqe.borrow_mut() = Some(cqe);

            match task.poll() {
                Poll::Pending => return,
                Poll::Ready(Ok(())) => (),
                Poll::Ready(Err(err)) => eprintln!("Client #{id} failed: {err:#}"),
            }

            self.clients.remove(&id);
        } else {
            eprintln!("Missing client #{id}");
        }
    }
}

struct Task {
    fut: Pin<Box<dyn Future<Output = Result<()>>>>,
    cqe: Rc<RefCell<Option<Cqe>>>,
}

impl Task {
    fn poll(&mut self) -> Poll<Result<()>> {
        let raw_waker = RawWaker::new(&(), &VTABLE_STUB);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);
        Pin::new(&mut self.fut).poll(&mut cx)
    }
}
