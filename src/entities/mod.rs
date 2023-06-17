
pub mod app;

pub mod machine;

pub mod org;

mod go_time;
pub use go_time::*;


#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProcessStat {
    pub pid: i32,
    pub stime: u64,
    pub rtime: u64,
    pub command: String,
    pub directory: String,
    pub cpu: u64,
    pub rss: u64,
    pub listen_sockets: Vec<ListenSocket>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ListenSocket {
    pub proto: String,
    pub address: String,
}