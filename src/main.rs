use std::{fmt, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

/// Proxy errors that keep their underlying causes for better debugging
#[derive(Debug)]
enum ProxyError {
    SocketCreateError(std::io::Error),
    SocketListenError(std::io::Error),
    ParsingError(String),
    SocketWriteError(std::io::Error),
    SocketReadError(std::io::Error),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::SocketCreateError(e) => write!(f, "Failed to create/connect socket: {}", e),
            ProxyError::SocketListenError(e) => write!(f, "Failed to accept connection: {}", e),
            ProxyError::ParsingError(msg)   => write!(f, "Failed to parse request: {}", msg),
            ProxyError::SocketWriteError(e)  => write!(f, "Failed to write to socket: {}", e),
            ProxyError::SocketReadError(e)   => write!(f, "Failed to read from socket: {}", e),
        }
    }
}

impl std::error::Error for ProxyError {}

struct ProxyServer {
    sock: TcpListener,
}

impl ProxyServer {
    pub async fn new(address: String) -> Result<ProxyServer, ProxyError> {
        println!("[+] Creating proxy server on {}", address);
        let sock = TcpListener::bind(address)
            .await
            .map_err(ProxyError::SocketCreateError)?;
        Ok(ProxyServer { sock })
    }

    async fn run(host: String, port: String, mut cstream: TcpStream, https: bool, first_req: Vec<u8>) -> Result<(), ProxyError> {

        let mut sstream = TcpStream::connect(format!("{}:{}", host, port))
            .await
            .map_err(ProxyError::SocketCreateError)?;

        if !https {
            sstream
                .write_all(&first_req)
                .await
                .map_err(ProxyError::SocketWriteError)?;
        } else {
            cstream
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .map_err(ProxyError::SocketWriteError)?;
        }

        // Bi-directional copy between client and server
        tokio::io::copy_bidirectional(&mut cstream, &mut sstream)
            .await
            .map_err(ProxyError::SocketReadError)?;

        Ok(())
    }

    async fn process(&self, mut stream: TcpStream) -> Result<(), ProxyError> {
        let mut buffer = [0; 4096];
        let bytes_read = stream
            .read(&mut buffer)
            .await
            .map_err(ProxyError::SocketReadError)?;

        if bytes_read == 0 {
            return Err(ProxyError::SocketReadError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Client closed connection",
            )));
        }

        let req = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

        // Get the request head
        let head = req
            .split("\r\n")
            .next()
            .ok_or_else(|| ProxyError::ParsingError("Missing request head".into()))?;

        // Get method
        let met = head
            .split(" ")
            .nth(0)
            .ok_or_else(|| ProxyError::ParsingError("Missing HTTP method".into()))?;

        // Get the URL
        let url = head
            .split(" ")
            .nth(1)
            .ok_or_else(|| ProxyError::ParsingError("Missing URL".into()))?;

        match met {
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" => {

                let info = Url::parse(url)
                    .map_err(|e| ProxyError::ParsingError(format!("Bad URL '{}': {}", url, e)))?;
                let host = info
                    .host_str()
                    .ok_or_else(|| ProxyError::ParsingError("Missing host".into()))?;
                let port = info
                    .port_or_known_default()
                    .ok_or_else(|| ProxyError::ParsingError("Missing port".into()))?;
                println!("[HTTP] {} {} (host={}, port={})", met, url, host, port);

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

                // Get the host
                let host = parts
                    .next()
                    .ok_or_else(|| ProxyError::ParsingError("Missing CONNECT host".into()))?;

                // Get the port
                let port = parts
                    .next()
                    .ok_or_else(|| ProxyError::ParsingError("Missing CONNECT port".into()))?;

                println!("[HTTPS] {} {} (host={}, port={})", met, url, host, port);

                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    true,
                    buffer[..bytes_read].to_vec()).await?;
            }

            _ => {
                return Err(ProxyError::ParsingError(format!(
                    "Unknown HTTP method: {}",
                    met
                )));
            }
        }

        Ok(())
    }

    pub async fn listen(self: Arc<Self>) -> Result<(), ProxyError> {
        println!("[+] Listening for incoming connections...");
        loop {
            let (stream, _) = self
                .sock
                .accept()
                .await
                .map_err(ProxyError::SocketListenError)?;


            let server_ref = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(err) = server_ref.process(stream).await {
                    eprintln!("[!] Error processing connection: {}", err);
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
