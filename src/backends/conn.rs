use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};

/// Read whatever is ready into `out`. Ok(false) means the peer has closed.
pub fn drain_read<S: Read>(
    stream: &mut S,
    out: &mut VecDeque<u8>,
    read_buf: usize,
) -> io::Result<bool> {
    let mut buf = vec![0u8; read_buf];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(false),
            Ok(n) => out.extend(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(true),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Push as much of `out` as the socket accepts now. Leftover stays queued and
/// goes out on the next writable notification. That leftover is the backpressure.
pub fn flush_write<S: Write>(stream: &mut S, out: &mut VecDeque<u8>) -> io::Result<()> {
    while !out.is_empty() {
        let (head, _) = out.as_slices();
        match stream.write(head) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                out.drain(..n);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// One client socket plus its pending echo bytes. Used by every readiness
/// backend that works with std sockets (nonblocking, select, poll, eventloop).
pub struct Conn {
    stream: TcpStream,
    out: VecDeque<u8>,
    read_buf: usize,
}

impl Conn {
    pub fn new(stream: TcpStream, read_buf: usize) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Conn {
            stream,
            out: VecDeque::new(),
            read_buf,
        })
    }

    pub fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    pub fn wants_write(&self) -> bool {
        !self.out.is_empty()
    }

    pub fn on_readable(&mut self) -> io::Result<bool> {
        drain_read(&mut self.stream, &mut self.out, self.read_buf)
    }

    pub fn on_writable(&mut self) -> io::Result<()> {
        flush_write(&mut self.stream, &mut self.out)
    }
}

/// Accept every pending connection without blocking.
pub fn accept_all(
    listener: &TcpListener,
    conns: &mut Vec<Conn>,
    read_buf: usize,
) -> io::Result<()> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => conns.push(Conn::new(stream, read_buf)?),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Drive one connection from its readiness. Returns false when it should be dropped.
pub fn serve_conn(c: &mut Conn, readable: bool, writable: bool) -> bool {
    if readable {
        match c.on_readable() {
            Ok(false) => return false,
            Ok(true) => {}
            Err(_) => return false,
        }
    }
    if writable || c.wants_write() {
        if c.on_writable().is_err() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Cursor;

    #[test]
    fn drain_reads_until_eof() {
        let mut src = Cursor::new(b"hello".to_vec());
        let mut out = VecDeque::new();
        let open = drain_read(&mut src, &mut out, 4).unwrap();
        assert!(!open, "a Cursor reports EOF, which we treat as closed");
        assert_eq!(out.iter().copied().collect::<Vec<u8>>(), b"hello");
    }

    #[test]
    fn flush_drains_the_queue() {
        let mut sink: Vec<u8> = Vec::new();
        let mut out: VecDeque<u8> = b"world".iter().copied().collect();
        flush_write(&mut sink, &mut out).unwrap();
        assert!(out.is_empty());
        assert_eq!(sink, b"world");
    }
}
