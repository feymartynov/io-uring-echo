#[macro_use]
extern crate anyhow;

mod buffer;
mod client;
mod common;
mod server;
mod utils;

use self::server::Server;

fn main() {
    Server::bind("0.0.0.0:3456").run();
}
