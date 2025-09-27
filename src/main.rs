use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc, thread,
};

use url::Url;

#[derive(Debug)]
enum ProxyError {
    SocketCreateError,
    SocketListenError,
    ParsingError,
    CloneError,
    ThreadError,
    SocketWriteError,
    SocketReadError,
}

struct ProxyServer {
    sock: TcpListener,
}

impl ProxyServer {
    pub fn new(address: String) -> Result<ProxyServer, ProxyError> {
        println!("[+] Creating proxy server on {}", address);
        let sock = TcpListener::bind(address).map_err(|_| ProxyError::SocketCreateError)?;
        Ok(ProxyServer { sock })
    }

    fn run(host: String, port: String, mut cstream: TcpStream, https: bool, first_req: Vec<u8>) -> Result<(), ProxyError> {
        let mut sstream = TcpStream::connect(format!("{}:{}", host, port))
            .map_err(|_| ProxyError::SocketCreateError)?;

        if !https {
            sstream
                .write_all(&first_req)
                .map_err(|_| ProxyError::SocketWriteError)?;
        } else {
            cstream
                .write(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .map_err(|_| ProxyError::SocketWriteError)?;
        }

        let mut cstream_clone = cstream.try_clone().map_err(|_| ProxyError::CloneError)?;
        let mut sstream_clone = sstream.try_clone().map_err(|_| ProxyError::CloneError)?;

        let c2s = thread::spawn(move || {
            let _ = std::io::copy(&mut cstream_clone, &mut sstream_clone);
        });

        let mut cstream_clone = cstream.try_clone().map_err(|_| ProxyError::CloneError)?;
        let mut sstream_clone = sstream.try_clone().map_err(|_| ProxyError::CloneError)?;

        let s2c = thread::spawn(move || {
            let _ = std::io::copy(&mut sstream_clone, &mut cstream_clone);
        });

        c2s.join().map_err(|_| ProxyError::ThreadError)?;
        s2c.join().map_err(|_| ProxyError::ThreadError)?;

        Ok(())
    }

    pub fn process(&self, mut stream: TcpStream) -> Result<(), ProxyError> {

        // Read the initial request from the client
        let mut buffer = [0; 4096];
        let bytes_read = stream
            .read(&mut buffer)
            .map_err(|_| ProxyError::SocketReadError)?;

        // Convert the request to a string for parsing
        let req = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
        let head = req
            .split("\r\n")
            .next()
            .ok_or(ProxyError::ParsingError)?;

        // Get the method and the URL the client is requesting
        let met = head.split(" ").nth(0).ok_or(ProxyError::ParsingError)?;
        let url = head.split(" ").nth(1).ok_or(ProxyError::ParsingError)?;

        match met {
            // HTTP request with full URL
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" => {
                let info  = Url::parse(url).map_err(|_| ProxyError::ParsingError)?;
                let host = info.host_str().ok_or(ProxyError::ParsingError)?;
                let port  = info.port_or_known_default().ok_or(ProxyError::ParsingError)?;
                println!("[HTTP] {} {} (host={},port={})", met, url, host, port);

                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    false,
                    buffer[..bytes_read].to_vec())?;
            }

            // HTTPS CONNECT request
            "CONNECT" => {
                let mut parts = url.split(':');
                let host = parts.next().ok_or(ProxyError::ParsingError)?;
                let port = parts.next().ok_or(ProxyError::ParsingError)?;
                println!("[HTTPS] {} {} (host={},port={})", met, url, host, port);

                // after this, just tunnel raw data (don’t parse again)
                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    true,
                    buffer[..bytes_read].to_vec())?;
            }

            _ => {
                println!("[!] Unknown method: {}", met);
                return Err(ProxyError::ParsingError);
            }
        }

        Ok(())
    }

    pub fn listen(self: Arc<Self>) -> Result<(), ProxyError> {
        println!("[+] Listening for incoming connections...");
        for res in self.sock.incoming() {
            match res {
                Ok(stream) => {
                    let server_ref = Arc::clone(&self);
                    std::thread::spawn(move || {
                        if let Err(err) = server_ref.process(stream) {
                            println!("[!] Error processing connection: {:?}", err);
                        }
                    });
                }
                Err(_) => return Err(ProxyError::SocketListenError),
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), ProxyError> {
    let server = Arc::new(ProxyServer::new("127.0.0.1:9999".to_string())?);
    server.listen()?;
    Ok(())
}
