use std::cell::RefCell;
use std::future::Future;
use std::os::fd::{AsRawFd, OwnedFd};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use anyhow::{Context as _, Result};
use io_uring::cqueue::Entry as Cqe;
use io_uring::opcode::{ReadFixed, WriteFixed};
use io_uring::types::Fd;
use io_uring::IoUring;

use crate::buffer::Guard as Buffer;
use crate::common::{Id, Route};
use crate::utils::Errno;

pub struct Client {
    id: Id,
    socket: OwnedFd,
    buffer: Buffer,
    ring: Rc<RefCell<IoUring>>,
    cqe: Rc<RefCell<Option<Cqe>>>,
}

impl Client {
    pub fn new(
        id: Id,
        socket: OwnedFd,
        buffer: Buffer,
        ring: Rc<RefCell<IoUring>>,
        cqe: Rc<RefCell<Option<Cqe>>>,
    ) -> Self {
        Self {
            id,
            socket,
            buffer,
            ring,
            cqe,
        }
    }

    pub async fn handle(&mut self) -> Result<()> {
        loop {
            let buffer = self.read().await?;

            if let Ok(message) = std::str::from_utf8(buffer) {
                println!(
                    "Unicode message from client #{} of {} bytes: {}",
                    self.id,
                    buffer.len(),
                    message
                );
            } else {
                println!(
                    "Binary message from client #{} of {} bytes: {:02x?}",
                    self.id,
                    buffer.len(),
                    buffer
                );
            }

            self.write(buffer).await?;
        }
    }

    async fn read(&self) -> Result<&[u8]> {
        let sqe = ReadFixed::new(
            Fd(self.socket.as_raw_fd()),
            self.buffer.as_ref() as *const _ as *mut _,
            self.buffer.as_ref().len() as u32,
            self.buffer.idx(),
        )
        .build()
        .user_data(Route::Client(self.id).into());

        {
            let mut ring = self.ring.borrow_mut();
            unsafe { ring.submission().push(&sqe) }.context("Push read")?;
            ring.submit().context("Submit read")?;
        }

        let cqe = WaitEventFuture::new(Rc::clone(&self.cqe)).await;

        match cqe.result() {
            errno if errno < 0 => bail!("Read error: {}", Errno(-errno)),
            0 => bail!("Disconnected"),
            len => Ok(&self.buffer.as_ref()[..(len as usize)]),
        }
    }

    async fn write(&self, buffer: &[u8]) -> Result<()> {
        let sqe = WriteFixed::new(
            Fd(self.socket.as_raw_fd()),
            buffer as *const _ as *mut _,
            buffer.len() as u32,
            self.buffer.idx(),
        )
        .build()
        .user_data(Route::Client(self.id).into());

        {
            let mut ring = self.ring.borrow_mut();
            unsafe { ring.submission().push(&sqe) }.context("Push write")?;
            ring.submit().context("Submit write")?;
        }

        let cqe = WaitEventFuture::new(Rc::clone(&self.cqe)).await;

        match cqe.result() {
            errno if errno < 0 => bail!("Write error: {}", Errno(-errno)),
            0 => bail!("Disconnected"),
            len if len as usize == buffer.len() => Ok(()),
            len => bail!(
                "Incomplete message written: {} of {} bytes",
                len,
                buffer.len()
            ),
        }
    }
}

struct WaitEventFuture {
    cqe: Rc<RefCell<Option<Cqe>>>,
}

impl WaitEventFuture {
    fn new(cqe: Rc<RefCell<Option<Cqe>>>) -> Self {
        *cqe.borrow_mut() = None;
        Self { cqe }
    }
}

impl Future for WaitEventFuture {
    type Output = Cqe;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.cqe.borrow_mut().take() {
            None => Poll::Pending,
            Some(cqe) => Poll::Ready(cqe),
        }
    }
}
