use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

use crate::{Config, Server, ShutdownSignal};

/// Stage 1. Accept a connection, hand it to its own thread, repeat. The kernel
/// and the OS scheduler do all the multiplexing. One thread per client is the
/// simplest thing that works, and the thing every later stage is trying to avoid.
pub struct Blocking {
    listener: TcpListener,
}

impl Server for Blocking {
    fn bind(cfg: &Config) -> io::Result<Self> {
        Ok(Blocking {
            listener: TcpListener::bind((cfg.addr, cfg.port))?,
        })
    }

    fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has an address")
    }

    fn serve(self, shutdown: ShutdownSignal) -> io::Result<()> {
        for incoming in self.listener.incoming() {
            // stop() wakes this blocked accept with a throwaway connection.
            if shutdown.is_stopped() {
                break;
            }
            let stream = incoming?;
            thread::spawn(move || {
                let _ = echo(stream);
            });
        }
        Ok(())
    }
}

fn echo(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        stream.write_all(&buf[..n])?;
    }
}
