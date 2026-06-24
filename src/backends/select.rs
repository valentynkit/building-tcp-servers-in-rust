use std::io;
use std::net::{SocketAddr, TcpListener};
use std::os::fd::AsRawFd;

use crate::backends::conn::{accept_all, serve_conn, Conn};
use crate::{Config, Server, ShutdownSignal};

/// Stage 3. The first kernel readiness API. `select` takes a set of fds and
/// blocks until one is ready, so the loop stops spinning. The cost: the fd sets
/// are rebuilt every pass (O(n)) and an fd above FD_SETSIZE (1024) cannot fit.
pub struct Select {
    listener: TcpListener,
    read_buf: usize,
}

impl Server for Select {
    fn bind(cfg: &Config) -> io::Result<Self> {
        let listener = TcpListener::bind((cfg.addr, cfg.port))?;
        listener.set_nonblocking(true)?;
        Ok(Select {
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
        let listen_fd = self.listener.as_raw_fd();
        let mut conns: Vec<Conn> = Vec::new();

        while !shutdown.is_stopped() {
            let mut read_set: libc::fd_set = unsafe { std::mem::zeroed() };
            let mut write_set: libc::fd_set = unsafe { std::mem::zeroed() };
            let mut max_fd = listen_fd;
            unsafe {
                libc::FD_ZERO(&mut read_set);
                libc::FD_ZERO(&mut write_set);
                libc::FD_SET(listen_fd, &mut read_set);
            }
            for c in &conns {
                let fd = c.fd();
                unsafe { libc::FD_SET(fd, &mut read_set) };
                if c.wants_write() {
                    unsafe { libc::FD_SET(fd, &mut write_set) };
                }
                if fd > max_fd {
                    max_fd = fd;
                }
            }

            // 100 ms timeout so stop() is noticed even with no traffic.
            let mut timeout = libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000,
            };
            let n = unsafe {
                libc::select(
                    max_fd + 1,
                    &mut read_set,
                    &mut write_set,
                    std::ptr::null_mut(),
                    &mut timeout,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            conns.retain_mut(|c| {
                let fd = c.fd();
                let readable = unsafe { libc::FD_ISSET(fd, &read_set) };
                let writable = unsafe { libc::FD_ISSET(fd, &write_set) };
                serve_conn(c, readable, writable)
            });

            if unsafe { libc::FD_ISSET(listen_fd, &read_set) } {
                accept_all(&self.listener, &mut conns, self.read_buf)?;
            }
        }
        Ok(())
    }
}
