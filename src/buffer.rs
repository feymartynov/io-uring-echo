use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
pub struct BufferPool {
    data: Rc<Vec<u8>>,
    count: u16,
    size: u32,
    free_indexes: Rc<RefCell<Vec<u16>>>,
}

impl BufferPool {
    pub fn new(count: u16, size: u32) -> Self {
        Self {
            data: Rc::new(vec![0; count as usize * size as usize]),
            count,
            size,
            free_indexes: Rc::new(RefCell::new((0..count).collect::<Vec<_>>())),
        }
    }

    pub fn acquire(&self) -> Option<Guard> {
        let idx = self.free_indexes.borrow_mut().pop()?;
        let start = idx as usize * self.size as usize;
        let end = start + self.size as usize;

        Some(Guard {
            buffer: Rc::clone(&self.data),
            start,
            end,
            idx,
            free_indexes: Rc::clone(&self.free_indexes),
        })
    }

    pub fn iovecs(&self) -> Vec<libc::iovec> {
        let count = self.count as usize;
        let size = self.size as usize;
        let mut iovecs = Vec::with_capacity(count);

        for i in 0..count {
            let ptr = self.data[(i * size)..].as_ptr();

            iovecs.push(libc::iovec {
                iov_base: ptr as *const _ as *mut libc::c_void,
                iov_len: size,
            });
        }

        iovecs
    }
}

pub struct Guard {
    buffer: Rc<Vec<u8>>,
    start: usize,
    end: usize,
    idx: u16,
    free_indexes: Rc<RefCell<Vec<u16>>>,
}

impl Guard {
    pub fn idx(&self) -> u16 {
        self.idx
    }
}

impl AsRef<[u8]> for Guard {
    fn as_ref(&self) -> &[u8] {
        &self.buffer[self.start..self.end]
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.free_indexes.borrow_mut().push(self.idx);
    }
}
