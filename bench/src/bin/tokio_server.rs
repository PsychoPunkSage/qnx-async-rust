//! Echo server for the benchmark, on tokio: a current-thread runtime (one
//! thread, like the mio server) with one task per connection, every message
//! echoed back as soon as it arrives. Built twice, like the mio server: tokio
//! runs on whichever mio selector the build selects.
//!
//! Usage: `tokio_server <port>`. Runs until killed.

use std::io;
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Enough for 1000+ connections plus the listener and stdio.
const MAX_FDS: u64 = 4096;

fn main() -> io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("usage: tokio_server <port>")
        .parse()
        .expect("port");
    bench::raise_limits(MAX_FDS)?;

    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?
        .block_on(async move {
            let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
            loop {
                let (stream, _) = listener.accept().await?;
                stream.set_nodelay(true)?;
                tokio::spawn(echo(stream));
            }
        })
}

/// Echo until the peer closes or the connection fails.
async fn echo(mut stream: TcpStream) {
    let mut buf = vec![0; 16 * 1024];
    loop {
        let len = match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(len) => len,
        };
        if stream.write_all(&buf[..len]).await.is_err() {
            return;
        }
    }
}
