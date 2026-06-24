use std::net::{IpAddr, Ipv4Addr};

/// Everything a backend needs to stand up its listener.
#[derive(Clone, Debug)]
pub struct Config {
    pub addr: IpAddr,
    pub port: u16,
    pub read_buf: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 9999,
            read_buf: 4096,
        }
    }
}
