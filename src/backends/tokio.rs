use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use crate::{Config, Server, ShutdownSignal};

/// Stage 7. The reactor from stages 5 and 6 is now tokio's job. The loop is
/// written in async: accept, then spawn a task that copies the socket back into
/// itself. Each `.await` is a point where the runtime parks the task on the same
/// epoll/kqueue machinery and runs another. The trait stays sync; this backend
/// owns its runtime and blocks on it, so async never crosses the seam.
pub struct Tokio {
    listener: std::net::TcpListener,
}

impl Server for Tokio {
    fn bind(cfg: &Config) -> io::Result<Self> {
        let listener = std::net::TcpListener::bind((cfg.addr, cfg.port))?;
        listener.set_nonblocking(true)?;
        Ok(Tokio { listener })
    }

    fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has an address")
    }

    fn serve(self, shutdown: ShutdownSignal) -> io::Result<()> {
        let rt = ::tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let listener = ::tokio::net::TcpListener::from_std(self.listener)?;
            while !shutdown.is_stopped() {
                // Time-box accept so a stop() is noticed without a connection.
                match ::tokio::time::timeout(Duration::from_millis(100), listener.accept()).await {
                    Ok(Ok((stream, _))) => {
                        ::tokio::spawn(async move {
                            let _ = echo(stream).await;
                        });
                    }
                    Ok(Err(e)) => return Err(e),
                    Err(_elapsed) => continue,
                }
            }
            Ok(())
        })
    }
}

async fn echo(mut stream: ::tokio::net::TcpStream) -> io::Result<()> {
    let (mut reader, mut writer) = stream.split();
    ::tokio::io::copy(&mut reader, &mut writer).await?;
    Ok(())
}
