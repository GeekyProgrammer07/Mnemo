use std::net::SocketAddr;

use bytes::BytesMut;
use socket2::{Domain, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod resp;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr: SocketAddr = "127.0.0.1:6379".parse().unwrap();
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.set_recv_buffer_size(1024 * 1024)?;
    socket.set_send_buffer_size(1024 * 1024)?;
    // Let us rebind port 6379 right away after a restart. Without this the port
    // sits in TIME_WAIT for ~60s and `cargo run` fails with "address in use".
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    // Start queueing incoming connections; 1024 is how many may wait un-accepted.
    socket.listen(1024)?;
    // Tokio drives this socket itself, so reads must never block the runtime.
    socket.set_nonblocking(true)?;
    // Ping idle connections every so often so dead ones get noticed. If a client's
    // laptop loses wifi it never sends a FIN, and without this we would hold that
    // connection open forever waiting for bytes that will never come.
    socket.set_keepalive(true)?;

    let listener = TcpListener::from_std(socket.into())?;
    println!("listening on 127.0.0.1:6379");

    loop {
        // Wait here until somebody connects.
        let (mut stream, addr) = listener.accept().await?;
        println!("connected: {addr}");
        // Turn off Nagle's algorithm. It holds small writes back for a moment
        // hoping to batch them, which would add latency to every reply.
        stream.set_nodelay(true)?;
        // Hand this client off and go straight back to accepting.
        tokio::spawn(async move {
            // Grows on demand, so one big command doesn't cost several reads from kernel.
            // A fixed array would also erase a half-delivered command.
            // `read_buf()` appends existing buffer
            let mut buf = BytesMut::with_capacity(4096);

            loop {
                let n = match stream.read_buf(&mut buf).await {
                    Ok(0) => {
                        println!("closed: {addr}");
                        return;
                    }
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("read error from {addr}: {e}");
                        return;
                    }
                };

                println!("got {n} bytes: {:?}", &buf[..n]);

                // Hardcoded for now. The parser goes here next.
                if let Err(e) = stream.write_all(b"+PONG\r\n").await {
                    eprintln!("write error to {addr}: {e}");
                    return;
                }
            }
        });
    }
}
