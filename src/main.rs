use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Buf, BytesMut};
use socket2::{Domain, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::dispatch::dispatch;
use crate::resp::encoder::encode;
use crate::resp::parser::{ParseError, parse_command};
use crate::store::db::Db;

mod dispatch;
mod resp;
mod store;

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
    let store = Arc::new(Mutex::new(Db::default()));
    loop {
        // Wait here until somebody connects.
        let (mut stream, addr) = listener.accept().await?;
        println!("connected: {addr}");
        // Turn off Nagle's algorithm. It holds small writes back for a moment
        // hoping to batch them, which would add latency to every reply.
        stream.set_nodelay(true)?;

        let store_for_each_conn = Arc::clone(&store);
        // Hand this client off and go straight back to accepting.
        tokio::spawn(async move {
            // Grows on demand, so one big command doesn't cost several reads from kernel.
            // A fixed array would also erase a half-delivered command.
            // `read_buf()` appends existing buffer
            let mut inbox = BytesMut::with_capacity(4096);
            // How far into `inbox` the parser has already consumed.
            let mut parsed_upto = 0;
            loop {
                let _bytes_read = match stream.read_buf(&mut inbox).await {
                    Ok(0) => {
                        println!("closed: {addr}");
                        return;
                    }
                    Ok(count) => count,
                    Err(e) => {
                        eprintln!("read error from {addr}: {e}");
                        return;
                    }
                };

                let request = match parse_command(&inbox, &mut parsed_upto) {
                    Ok(frame) => frame,
                    Err(ParseError::Incomplete) => continue,
                    Err(ParseError::Protocol(msg)) => {
                        eprintln!("protocol error from {addr}: {msg}");
                        return;
                    }
                };

                let mut guard = store_for_each_conn.lock().await;
                let reply = dispatch(request, &mut guard);
                let reply_bytes = encode(&reply);
                // `write_all`, not `write`: a plain write may send only part of the
                // buffer and report how much, which would truncate the reply.
                if let Err(e) = stream.write_all(&reply_bytes).await {
                    eprintln!("write error to {addr}: {e}");
                    return;
                }
                inbox.advance(parsed_upto);
                parsed_upto = 0;
            }
        });
    }
}
