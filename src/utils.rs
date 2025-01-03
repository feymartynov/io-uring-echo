use std::ffi::CStr;
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub struct Errno(pub libc::c_int);

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let err = unsafe { CStr::from_ptr(libc::strerror(self.0)) };
        write!(f, "{} ({})", err.to_str().map_err(|_| fmt::Error)?, self.0)
    }
}
