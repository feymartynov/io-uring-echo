#[macro_use]
extern crate anyhow;

mod buffer;
mod client;
mod common;
mod server;
mod utils;

use anyhow::Result;

use self::server::Server;

fn main() -> Result<()> {
    let server = Server::bind("0.0.0.0:3456")?;
    server.run()
}
