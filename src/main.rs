use std::{sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::{TcpListener, TcpStream},
};
use url::Url;

#[derive(Debug)]
enum ProxyError {
    SocketCreateError,
    SocketListenError,
    ParsingError,
    SocketWriteError,
    SocketReadError,
}

struct ProxyServer {
    sock: TcpListener,
}

impl ProxyServer {
    pub async fn new(address: String) -> Result<ProxyServer, ProxyError> {
        println!("[+] Creating proxy server on {}", address);
        let sock = TcpListener::bind(address)
            .await
            .map_err(|_| ProxyError::SocketCreateError)?;
        Ok(ProxyServer { sock })
    }

    async fn run(host: String, port: String, mut cstream: TcpStream, https: bool, first_req: Vec<u8>) -> Result<(), ProxyError> {

        // Bi-directional copy until EOF on either side
        let mut sstream = TcpStream::connect(format!("{}:{}", host, port))
            .await
            .map_err(|_| ProxyError::SocketCreateError)?;

        if !https {
            sstream
                .write_all(&first_req)
                .await
                .map_err(|_| ProxyError::SocketWriteError)?;
        } else {
            cstream
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .map_err(|_| ProxyError::SocketWriteError)?;
        }

        // Bi-directional copy between client and server
        tokio::io::copy_bidirectional(&mut cstream, &mut sstream)
            .await
            .map_err(|_| ProxyError::SocketReadError)?;

        Ok(())
    }

    async fn process(&self, mut stream: TcpStream) -> Result<(), ProxyError> {
        let mut buffer = [0; 4096];
        let bytes_read = stream
            .read(&mut buffer)
            .await
            .map_err(|_| ProxyError::SocketReadError)?;

        let req = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
        let head = req.split("\r\n").next().ok_or(ProxyError::ParsingError)?;

        let met = head.split(" ").nth(0).ok_or(ProxyError::ParsingError)?;
        let url = head.split(" ").nth(1).ok_or(ProxyError::ParsingError)?;

        match met {
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" => {
                let info = Url::parse(url).map_err(|_| ProxyError::ParsingError)?;
                let host = info.host_str().ok_or(ProxyError::ParsingError)?;
                let port = info.port_or_known_default().ok_or(ProxyError::ParsingError)?;
                println!("[HTTP] {} {} (host={},port={})", met, url, host, port);

                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    false,
                    buffer[..bytes_read].to_vec(),
                )
                .await?;
            }

            "CONNECT" => {
                let mut parts = url.split(':');
                let host = parts.next().ok_or(ProxyError::ParsingError)?;
                let port = parts.next().ok_or(ProxyError::ParsingError)?;
                println!("[HTTPS] {} {} (host={},port={})", met, url, host, port);

                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    true,
                    buffer[..bytes_read].to_vec(),
                )
                .await?;
            }

            _ => {
                println!("[!] Unknown method: {}", met);
                return Err(ProxyError::ParsingError);
            }
        }

        Ok(())
    }

    pub async fn listen(self: Arc<Self>) -> Result<(), ProxyError> {
        println!("[+] Listening for incoming connections...");
        loop {
            let (stream, _) = self.sock.accept().await.map_err(|_| ProxyError::SocketListenError)?;
            let server_ref = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(err) = server_ref.process(stream).await {
                    println!("[!] Error processing connection: {:?}", err);
                }
            });
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    let server = Arc::new(ProxyServer::new("127.0.0.1:9999".to_string()).await?);
    server.listen().await?;
    Ok(())
}
