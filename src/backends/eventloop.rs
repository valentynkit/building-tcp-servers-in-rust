//! Stage 5. Register each fd with the kernel once, then ask only for the fds
//! that are ready (O(ready), not O(n)). epoll on Linux, kqueue on the BSDs and
//! macOS. The shape is identical: create the queue, add interests, wait, react,
//! adjust interest when there are queued bytes to flush.

#[cfg(target_os = "linux")]
mod epoll {
    use std::collections::HashMap;
    use std::io;
    use std::net::{SocketAddr, TcpListener};
    use std::os::fd::{AsRawFd, RawFd};

    use crate::backends::conn::{serve_conn, Conn};
    use crate::{Config, Server, ShutdownSignal};

    pub struct EventLoop {
        listener: TcpListener,
        read_buf: usize,
    }

    impl Server for EventLoop {
        fn bind(cfg: &Config) -> io::Result<Self> {
            let listener = TcpListener::bind((cfg.addr, cfg.port))?;
            listener.set_nonblocking(true)?;
            Ok(EventLoop {
                listener,
                read_buf: cfg.read_buf,
            })
        }

        fn local_addr(&self) -> SocketAddr {
            self.listener
                .local_addr()
                .expect("a bound listener has an address")
        }

        fn serve(self, shutdown: ShutdownSignal) -> io::Result<()> {
            let epfd = unsafe { libc::epoll_create1(0) };
            if epfd < 0 {
                return Err(io::Error::last_os_error());
            }
            let listen_fd = self.listener.as_raw_fd();
            // If the first registration fails the queue is still open, so close
            // it by hand rather than leaking the fd on an early return.
            if let Err(e) = ctl(epfd, libc::EPOLL_CTL_ADD, listen_fd, libc::EPOLLIN as u32) {
                unsafe { libc::close(epfd) };
                return Err(e);
            }

            let mut conns: HashMap<RawFd, Conn> = HashMap::new();
            let mut events = vec![libc::epoll_event { events: 0, u64: 0 }; 1024];

            // Every exit from this loop falls through to close(epfd) below, so no
            // arm uses a bare `?` that would skip it.
            let result = loop {
                if shutdown.is_stopped() {
                    break Ok(());
                }
                let n = unsafe {
                    libc::epoll_wait(epfd, events.as_mut_ptr(), events.len() as i32, 100)
                };
                if n < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    break Err(err);
                }
                let mut loop_err = None;
                for ev in &events[..n as usize] {
                    let fd = ev.u64 as RawFd;
                    if fd == listen_fd {
                        if let Err(e) = accept(epfd, &self.listener, &mut conns, self.read_buf) {
                            loop_err = Some(e);
                            break;
                        }
                        continue;
                    }
                    let keep = match conns.get_mut(&fd) {
                        Some(c) => {
                            let readable = ev.events
                                & (libc::EPOLLIN as u32
                                    | libc::EPOLLHUP as u32
                                    | libc::EPOLLERR as u32)
                                != 0;
                            let writable = ev.events & libc::EPOLLOUT as u32 != 0;
                            serve_conn(c, readable, writable)
                        }
                        None => continue,
                    };
                    if !keep {
                        unsafe {
                            libc::epoll_ctl(epfd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut())
                        };
                        conns.remove(&fd);
                    } else {
                        let want_write = conns[&fd].wants_write();
                        let mut interest = libc::EPOLLIN as u32;
                        if want_write {
                            interest |= libc::EPOLLOUT as u32;
                        }
                        // A MOD failure means the kernel-side fd is gone;
                        // drop this connection and continue the loop.
                        if ctl(epfd, libc::EPOLL_CTL_MOD, fd, interest).is_err() {
                            unsafe {
                                libc::epoll_ctl(epfd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut())
                            };
                            conns.remove(&fd);
                        }
                    }
                }
                if let Some(e) = loop_err {
                    break Err(e);
                }
            };
            unsafe { libc::close(epfd) };
            result
        }
    }

    fn ctl(epfd: RawFd, op: i32, fd: RawFd, events: u32) -> io::Result<()> {
        let mut ev = libc::epoll_event {
            events,
            u64: fd as u64,
        };
        let r = unsafe { libc::epoll_ctl(epfd, op, fd, &mut ev) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn accept(
        epfd: RawFd,
        listener: &TcpListener,
        conns: &mut HashMap<RawFd, Conn>,
        read_buf: usize,
    ) -> io::Result<()> {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let conn = Conn::new(stream, read_buf)?;
                    let fd = conn.fd();
                    ctl(epfd, libc::EPOLL_CTL_ADD, fd, libc::EPOLLIN as u32)?;
                    conns.insert(fd, conn);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use epoll::EventLoop;

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
mod kqueue {
    use std::collections::HashMap;
    use std::io;
    use std::net::{SocketAddr, TcpListener};
    use std::os::fd::{AsRawFd, RawFd};

    use crate::backends::conn::{serve_conn, Conn};
    use crate::{Config, Server, ShutdownSignal};

    pub struct EventLoop {
        listener: TcpListener,
        read_buf: usize,
    }

    impl Server for EventLoop {
        fn bind(cfg: &Config) -> io::Result<Self> {
            let listener = TcpListener::bind((cfg.addr, cfg.port))?;
            listener.set_nonblocking(true)?;
            Ok(EventLoop {
                listener,
                read_buf: cfg.read_buf,
            })
        }

        fn local_addr(&self) -> SocketAddr {
            self.listener
                .local_addr()
                .expect("a bound listener has an address")
        }

        fn serve(self, shutdown: ShutdownSignal) -> io::Result<()> {
            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                return Err(io::Error::last_os_error());
            }
            let listen_fd = self.listener.as_raw_fd();
            // The queue is open already, so close it on an early registration
            // failure instead of leaking the fd.
            if let Err(e) = change(kq, listen_fd, libc::EVFILT_READ, libc::EV_ADD) {
                unsafe { libc::close(kq) };
                return Err(e);
            }

            let mut conns: HashMap<RawFd, Conn> = HashMap::new();
            let mut events: Vec<libc::kevent> = (0..1024).map(|_| empty_kevent()).collect();

            // Every exit from this loop falls through to close(kq) below, so no
            // arm uses a bare `?` that would skip it.
            let result = loop {
                if shutdown.is_stopped() {
                    break Ok(());
                }
                // 100 ms wait so a stop() between events is still seen promptly.
                let timeout = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 100_000_000,
                };
                let n = unsafe {
                    libc::kevent(
                        kq,
                        std::ptr::null(),
                        0,
                        events.as_mut_ptr(),
                        events.len() as i32,
                        &timeout,
                    )
                };
                if n < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    break Err(err);
                }
                let mut loop_err = None;
                for ev in &events[..n as usize] {
                    let fd = ev.ident as RawFd;
                    if fd == listen_fd {
                        if let Err(e) = accept(kq, &self.listener, &mut conns, self.read_buf) {
                            loop_err = Some(e);
                            break;
                        }
                        continue;
                    }
                    // EV_EOF on the read filter is the peer closing; let
                    // serve_conn read the final bytes and notice the close.
                    let readable = ev.filter == libc::EVFILT_READ || ev.flags & libc::EV_EOF != 0;
                    let writable = ev.filter == libc::EVFILT_WRITE;
                    let keep = match conns.get_mut(&fd) {
                        Some(c) => serve_conn(c, readable, writable),
                        None => continue,
                    };
                    if !keep {
                        let _ = change(kq, fd, libc::EVFILT_READ, libc::EV_DELETE);
                        let _ = change(kq, fd, libc::EVFILT_WRITE, libc::EV_DELETE);
                        conns.remove(&fd);
                    } else if conns[&fd].wants_write() {
                        // An EV_ADD failure means the fd is already gone;
                        // drop this connection and continue the loop.
                        if change(kq, fd, libc::EVFILT_WRITE, libc::EV_ADD).is_err() {
                            let _ = change(kq, fd, libc::EVFILT_READ, libc::EV_DELETE);
                            let _ = change(kq, fd, libc::EVFILT_WRITE, libc::EV_DELETE);
                            conns.remove(&fd);
                        }
                    } else {
                        // Drop the write filter when the queue drains. EV_DELETE
                        // on a filter that was never added returns ENOENT, which
                        // we ignore.
                        let _ = change(kq, fd, libc::EVFILT_WRITE, libc::EV_DELETE);
                    }
                }
                if let Some(e) = loop_err {
                    break Err(e);
                }
            };
            unsafe { libc::close(kq) };
            result
        }
    }

    fn empty_kevent() -> libc::kevent {
        libc::kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        }
    }

    fn change(kq: RawFd, fd: RawFd, filter: i16, flags: u16) -> io::Result<()> {
        let kev = libc::kevent {
            ident: fd as libc::uintptr_t,
            filter,
            flags,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let r = unsafe { libc::kevent(kq, &kev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn accept(
        kq: RawFd,
        listener: &TcpListener,
        conns: &mut HashMap<RawFd, Conn>,
        read_buf: usize,
    ) -> io::Result<()> {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let conn = Conn::new(stream, read_buf)?;
                    let fd = conn.fd();
                    change(kq, fd, libc::EVFILT_READ, libc::EV_ADD)?;
                    conns.insert(fd, conn);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
pub use kqueue::EventLoop;
