use std::os::fd::{AsRawFd, RawFd};

/// Echo stays off while this lives, so a secret typed at a terminal is never shown.
pub struct Unechoed {
    fd: RawFd,
    was: libc::termios,
}

impl Unechoed {
    #[allow(unsafe_code)]
    pub fn on(input: &impl AsRawFd) -> Option<Self> {
        let fd = input.as_raw_fd();
        let mut was = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `tcgetattr` fills `was` whenever it answers 0, and only then is it read.
        let was = unsafe {
            if libc::tcgetattr(fd, was.as_mut_ptr()) != 0 {
                return None;
            }
            was.assume_init()
        };
        let mut quiet = was;
        quiet.c_lflag &= !libc::ECHO;
        // SAFETY: `quiet` is a complete termios read from this descriptor.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const quiet) } != 0 {
            return None;
        }
        Some(Self { fd, was })
    }
}

impl Drop for Unechoed {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: restores the termios this guard read from the same descriptor.
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &raw const self.was);
        }
    }
}
