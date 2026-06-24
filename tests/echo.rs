use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use building_tcp_servers_in_rust::{Config, Server, ShutdownSignal};

/// A backend running on its own thread, plus the handle to stop it.
struct Running {
    addr: SocketAddr,
    shutdown: ShutdownSignal,
    handle: Option<JoinHandle<()>>,
}

fn start<S: Server>() -> Running {
    let cfg = Config {
        read_buf: 4096,
        port: 0,
        ..Config::default()
    };
    let server = S::bind(&cfg).expect("bind");
    let addr = server.local_addr();
    let shutdown = ShutdownSignal::new();
    let sd = shutdown.clone();
    let handle = thread::spawn(move || {
        let _ = server.serve(sd);
    });
    Running {
        addr,
        shutdown,
        handle: Some(handle),
    }
}

impl Running {
    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(self.addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.stop();
        // Wake a backend blocked in accept. Harmless to the others.
        let _ = TcpStream::connect(self.addr);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn run_echo_one<S: Server>() {
    let srv = start::<S>();
    let mut c = srv.connect();
    c.write_all(b"hello").unwrap();
    let mut buf = [0u8; 5];
    c.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}

pub fn run_echo_concurrent<S: Server>() {
    let srv = start::<S>();
    let mut clients = Vec::new();
    for i in 0..64u8 {
        let mut c = srv.connect();
        let msg = [i, i, i, i];
        c.write_all(&msg).unwrap();
        clients.push((c, msg));
    }
    for (mut c, msg) in clients {
        let mut buf = [0u8; 4];
        c.read_exact(&mut buf).unwrap();
        assert_eq!(buf, msg, "each client gets its own bytes back");
    }
}

pub fn run_echo_large<S: Server>() {
    let srv = start::<S>();
    let payload: Vec<u8> = (0..(1usize << 20)).map(|i| i as u8).collect();
    let mut c = srv.connect();

    let writer = {
        let mut w = c.try_clone().unwrap();
        let p = payload.clone();
        thread::spawn(move || w.write_all(&p).unwrap())
    };

    let mut got = vec![0u8; payload.len()];
    c.read_exact(&mut got).unwrap();
    writer.join().unwrap();
    assert_eq!(got, payload, "a 1 MiB payload echoes back intact");
}

pub fn run_survives_mid_disconnect<S: Server>() {
    let srv = start::<S>();
    {
        let mut c = srv.connect();
        c.write_all(b"partial").unwrap();
        // Drop without reading. The server may try to echo into a gone socket.
    }
    // A fresh client must still be served, proving the server did not die.
    let mut c = srv.connect();
    c.write_all(b"ok").unwrap();
    let mut buf = [0u8; 2];
    c.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ok");
}

macro_rules! backend_suite {
    ($name:ident, $ty:ty) => {
        mod $name {
            use super::*;
            use building_tcp_servers_in_rust::backends::*;

            #[test]
            fn echo_one() {
                run_echo_one::<$ty>();
            }
            #[test]
            fn echo_concurrent() {
                run_echo_concurrent::<$ty>();
            }
            #[test]
            fn echo_large() {
                run_echo_large::<$ty>();
            }
            #[test]
            fn survives_mid_disconnect() {
                run_survives_mid_disconnect::<$ty>();
            }
        }
    };
}

backend_suite!(blocking, Blocking);
