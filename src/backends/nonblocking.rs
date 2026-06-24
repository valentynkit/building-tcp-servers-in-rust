use std::io;
use std::net::{SocketAddr, TcpListener};

use crate::backends::conn::Conn;
use crate::{Config, Server, ShutdownSignal};

/// Stage 2. The listener and every client are nonblocking, so a busy loop can
/// juggle them all on one thread. The catch: with no readiness API, the only way
/// to find work is to ask every socket every pass, which pins a core at 100%.
pub struct Nonblocking {
    listener: TcpListener,
    read_buf: usize,
}

impl Server for Nonblocking {
    fn bind(cfg: &Config) -> io::Result<Self> {
        let listener = TcpListener::bind((cfg.addr, cfg.port))?;
        listener.set_nonblocking(true)?;
        Ok(Nonblocking {
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
        let mut conns: Vec<Conn> = Vec::new();
        while !shutdown.is_stopped() {
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => conns.push(Conn::new(stream, self.read_buf)?),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            conns.retain_mut(service);
        }
        Ok(())
    }
}

fn service(c: &mut Conn) -> bool {
    match c.on_readable() {
        Ok(false) => return false,
        Ok(true) => {}
        Err(_) => return false,
    }
    c.on_writable().is_ok()
}
