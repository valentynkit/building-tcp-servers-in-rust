use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use mio::event::Event;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

use crate::backends::conn::{drain_read, flush_write};
use crate::{Config, Server, ShutdownSignal};

const LISTENER: Token = Token(0);

/// Stage 6. The same register-once, wait-for-ready loop as stage 5, but mio
/// owns the epoll/kqueue difference behind one safe API. No unsafe here: mio
/// wraps exactly the syscalls stage 5 wrote by hand.
pub struct Mio {
    listener: TcpListener,
    read_buf: usize,
}

impl Server for Mio {
    fn bind(cfg: &Config) -> io::Result<Self> {
        let addr = SocketAddr::new(cfg.addr, cfg.port);
        let listener = TcpListener::bind(addr)?;
        Ok(Mio {
            listener,
            read_buf: cfg.read_buf,
        })
    }

    fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has an address")
    }

    fn serve(mut self, shutdown: ShutdownSignal) -> io::Result<()> {
        let mut poll = Poll::new()?;
        let mut events = Events::with_capacity(1024);
        poll.registry()
            .register(&mut self.listener, LISTENER, Interest::READABLE)?;

        let mut conns: HashMap<Token, MioConn> = HashMap::new();
        let mut next = 1usize;

        while !shutdown.is_stopped() {
            poll.poll(&mut events, Some(Duration::from_millis(100)))?;
            for event in events.iter() {
                match event.token() {
                    LISTENER => loop {
                        match self.listener.accept() {
                            Ok((mut stream, _)) => {
                                let token = Token(next);
                                next += 1;
                                poll.registry()
                                    .register(&mut stream, token, Interest::READABLE)?;
                                conns.insert(token, MioConn::new(stream, self.read_buf));
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) => return Err(e),
                        }
                    },
                    token => {
                        let drop_it = match conns.get_mut(&token) {
                            Some(c) => c.ready(event),
                            None => continue,
                        };
                        if drop_it {
                            if let Some(mut c) = conns.remove(&token) {
                                let _ = poll.registry().deregister(&mut c.stream);
                            }
                        } else if let Some(c) = conns.get_mut(&token) {
                            let interest = if c.wants_write() {
                                Interest::READABLE | Interest::WRITABLE
                            } else {
                                Interest::READABLE
                            };
                            poll.registry().reregister(&mut c.stream, token, interest)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct MioConn {
    stream: TcpStream,
    out: VecDeque<u8>,
    read_buf: usize,
}

impl MioConn {
    fn new(stream: TcpStream, read_buf: usize) -> Self {
        MioConn {
            stream,
            out: VecDeque::new(),
            read_buf,
        }
    }

    fn wants_write(&self) -> bool {
        !self.out.is_empty()
    }

    /// Returns true when the connection should be dropped.
    fn ready(&mut self, event: &Event) -> bool {
        if event.is_readable() {
            match drain_read(&mut self.stream, &mut self.out, self.read_buf) {
                Ok(false) | Err(_) => return true,
                Ok(true) => {}
            }
        }
        if event.is_writable() || self.wants_write() {
            if flush_write(&mut self.stream, &mut self.out).is_err() {
                return true;
            }
        }
        false
    }
}
