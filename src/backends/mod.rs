use std::io;
use std::str::FromStr;

use crate::{Config, Server, ShutdownSignal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Blocking,
    Nonblocking,
    Select,
    Poll,
    EventLoop,
    Mio,
    Tokio,
}

impl Backend {
    pub const NAMES: &'static [&'static str] = &[
        "blocking",
        "nonblocking",
        "select",
        "poll",
        "eventloop",
        "mio",
        "tokio",
    ];

    pub fn launch(self, cfg: &Config, shutdown: ShutdownSignal) -> io::Result<()> {
        match self {
            Backend::Blocking => launch::<Blocking>(cfg, shutdown),
            Backend::Nonblocking => launch::<Nonblocking>(cfg, shutdown),
            Backend::Select => launch::<Select>(cfg, shutdown),
            Backend::Poll => launch::<Poll>(cfg, shutdown),
            Backend::EventLoop => launch::<EventLoop>(cfg, shutdown),
            Backend::Mio => launch::<Mio>(cfg, shutdown),
            Backend::Tokio => launch::<Tokio>(cfg, shutdown),
        }
    }
}

impl FromStr for Backend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "blocking" => Backend::Blocking,
            "nonblocking" => Backend::Nonblocking,
            "select" => Backend::Select,
            "poll" => Backend::Poll,
            "eventloop" => Backend::EventLoop,
            "mio" => Backend::Mio,
            "tokio" => Backend::Tokio,
            other => return Err(format!("unknown backend: {other}")),
        })
    }
}

fn launch<S: Server>(cfg: &Config, shutdown: ShutdownSignal) -> io::Result<()> {
    let server = S::bind(cfg)?;
    eprintln!("listening on {}", server.local_addr());
    server.serve(shutdown)
}

pub mod conn;

mod blocking;
mod eventloop;
mod mio;
mod nonblocking;
mod poll;
mod select;
mod tokio;

pub use blocking::Blocking;
pub use eventloop::EventLoop;
pub use mio::Mio;
pub use nonblocking::Nonblocking;
pub use poll::Poll;
pub use select::Select;
pub use tokio::Tokio;
