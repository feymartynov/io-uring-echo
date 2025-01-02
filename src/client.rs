use std::cell::RefCell;
use std::os::fd::{AsRawFd, OwnedFd};
use std::rc::Rc;

use io_uring::opcode::{ReadFixed, WriteFixed};
use io_uring::types::Fd;
use io_uring::IoUring;

use crate::buffer::Guard as Buffer;
use crate::common::{Id, Route};

pub type IsComplete = bool;

pub struct Client {
    id: Id,
    socket: OwnedFd,
    buffer: Buffer,
    ring: Rc<RefCell<IoUring>>,
    prev_message_len: u32,
}

impl Client {
    pub fn new(id: Id, socket: OwnedFd, buffer: Buffer, ring: Rc<RefCell<IoUring>>) -> Self {
        Self {
            id,
            socket,
            buffer,
            ring,
            prev_message_len: 0,
        }
    }

    pub fn submit_read(&mut self) {
        let sqe = ReadFixed::new(
            Fd(self.socket.as_raw_fd()),
            self.buffer.as_ref() as *const _ as *mut _,
            self.buffer.as_ref().len() as u32,
            self.buffer.idx(),
        )
        .build()
        .user_data(Route::ClientRead(self.id).into());

        let mut ring = self.ring.borrow_mut();
        unsafe { ring.submission().push(&sqe) }.expect("push read");
        ring.submit().expect("submit read");
    }

    pub fn complete_read(&mut self, len: u32) -> IsComplete {
        if len > 0 {
            let id = self.id;
            let buffer = &self.buffer.as_ref()[..(len as usize)];

            if let Ok(message) = std::str::from_utf8(buffer) {
                println!("Unicode message from client #{id} of {len} bytes: {message}",);
            } else {
                println!("Binary message from client #{id} of {len} bytes: {buffer:02x?}",);
            }

            self.prev_message_len = len;
            self.submit_write(len);
            false
        } else {
            println!("Client #{} disconnected", self.id);
            true
        }
    }

    pub fn submit_write(&mut self, len: u32) {
        let sqe = WriteFixed::new(
            Fd(self.socket.as_raw_fd()),
            self.buffer.as_ref() as *const _ as *mut _,
            len,
            self.buffer.idx(),
        )
        .build()
        .user_data(Route::ClientWrite(self.id).into());

        let mut ring = self.ring.borrow_mut();
        unsafe { ring.submission().push(&sqe) }.expect("push write");
        ring.submit().expect("submit write");
    }

    pub fn complete_write(&mut self, len: u32) -> IsComplete {
        if len == self.prev_message_len {
            false
        } else if len > 0 {
            eprintln!(
                "Only {} of {} bytes written for client #{}",
                len, self.prev_message_len, self.id
            );

            true
        } else {
            println!("Client #{} disconnected", self.id);
            true
        }
    }
}
