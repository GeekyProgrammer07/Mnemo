use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod resp;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    println!("listening on 127.0.0.1:6379");

    loop {
        // Wait here until somebody connects.
        let (mut stream, addr) = listener.accept().await?;
        println!("connected: {addr}");

        // Hand this client off and go straight back to accepting.
        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];

            loop {
                let n = match stream.read(&mut buf).await {
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
