use std::io;
use std::net::{SocketAddr, TcpListener};
use std::os::fd::AsRawFd;

use crate::backends::conn::{accept_all, serve_conn, Conn};
use crate::{Config, Server, ShutdownSignal};

/// Stage 4. The same readiness idea as select with a cleaner interface: an array
/// of pollfd instead of bitmask sets, so there is no FD_SETSIZE cap. Still O(n),
/// because the whole array is handed to the kernel and scanned every pass.
pub struct Poll {
    listener: TcpListener,
    read_buf: usize,
}

impl Server for Poll {
    fn bind(cfg: &Config) -> io::Result<Self> {
        let listener = TcpListener::bind((cfg.addr, cfg.port))?;
        listener.set_nonblocking(true)?;
        Ok(Poll {
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
            // Index 0 is the listener; conns[i] maps to fds[i + 1].
            let mut fds: Vec<libc::pollfd> = Vec::with_capacity(conns.len() + 1);
            fds.push(libc::pollfd {
                fd: listen_fd,
                events: libc::POLLIN,
                revents: 0,
            });
            for c in &conns {
                let mut events = libc::POLLIN;
                if c.wants_write() {
                    events |= libc::POLLOUT;
                }
                fds.push(libc::pollfd {
                    fd: c.fd(),
                    events,
                    revents: 0,
                });
            }

            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 100) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            // Service existing connections first so the index mapping holds,
            // then accept new ones onto the end.
            let mut i = 0;
            conns.retain_mut(|c| {
                let revents = fds[i + 1].revents;
                i += 1;
                let readable =
                    revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0;
                let writable = revents & libc::POLLOUT != 0;
                serve_conn(c, readable, writable)
            });

            if fds[0].revents & libc::POLLIN != 0 {
                accept_all(&self.listener, &mut conns, self.read_buf)?;
            }
        }
        Ok(())
    }
}
