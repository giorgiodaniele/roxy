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
        // Create socket
        let sock = TcpListener::bind(address).map_err(|_| ProxyError::SocketCreateError)?;
        Ok(ProxyServer { sock })
    }

    fn run(host: String, port: String, mut cstream: TcpStream, https: bool, first_req: Vec<u8>) -> Result<(), ProxyError> {
        // Connect remote server
        let mut sstream = TcpStream::connect(format!("{}:{}", host, port))
            .map_err(|_| ProxyError::SocketCreateError)?;

        if https == false {
            // Send the first HTTP request (already read from client) to the server
            sstream
                .write_all(&first_req)
                .map_err(|_| ProxyError::SocketWriteError)?;
        } else {
            // Send the client the tunnel is ready
            cstream
                .write(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .map_err(|_| ProxyError::SocketWriteError)?;
        }

        // Clone streams for bidirectional forwarding
        let mut cstream_clone = cstream.try_clone().map_err(|_| ProxyError::CloneError)?;
        let mut sstream_clone = sstream.try_clone().map_err(|_| ProxyError::CloneError)?;

        // Forward client -> server in a separate thread
        let c2s = thread::spawn(move || {
            let _ = std::io::copy(&mut cstream_clone, &mut sstream_clone);
        });

        // Clone streams for bidirectional forwarding
        let mut cstream_clone = cstream.try_clone().map_err(|_| ProxyError::CloneError)?;
        let mut sstream_clone = sstream.try_clone().map_err(|_| ProxyError::CloneError)?;

        // Forward server -> client (in current thread)
        let s2c = thread::spawn(move || {
            let _ = std::io::copy(&mut sstream_clone, &mut cstream_clone);
        });

        // Await for threads to finish
        c2s.join().map_err(|_| ProxyError::ThreadError)?;
        s2c.join().map_err(|_| ProxyError::ThreadError)?;

        Ok(())

    }

    pub fn process(&self, mut stream: TcpStream) -> Result<(), ProxyError> {
        // Read first 4096 bytes
        let mut buffer = [0; 4096];
        let bytes_read = stream
            .read(&mut buffer)
            .map_err(|_| ProxyError::SocketReadError)?;

        // Convert bytes into string and get the header
        let req = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
        let head = req
            .split("\r\n")
            .next()
            .ok_or(ProxyError::ParsingError)?;

        // From the head, get the method and the URL/target
        let met = head.split(" ").nth(0).ok_or(ProxyError::ParsingError)?;
        let url = head.split(" ").nth(1).ok_or(ProxyError::ParsingError)?;


        match met {
            // HTTP
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" => {
                let info  = Url::parse(url).map_err(|_| ProxyError::ParsingError)?;
                let host = info.host_str().ok_or(ProxyError::ParsingError)?;
                let port  = info.port_or_known_default().ok_or(ProxyError::ParsingError)?;

                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    false,
                    buffer[..bytes_read].to_vec())?;
            }

            // HTTPS
            "CONNECT" => {
                let mut parts = url.split(':');
                let host = parts.next().ok_or(ProxyError::ParsingError)?;
                let port = parts.next().ok_or(ProxyError::ParsingError)?;
                
                ProxyServer::run(
                    host.to_string(),
                    port.to_string(),
                    stream,
                    true,
                    buffer[..bytes_read].to_vec())?;
            }

            _ => return Err(ProxyError::ParsingError),
        }

        Ok(())
    }


    pub fn listen(self: Arc<Self>) -> Result<(), ProxyError> {
        for res in self.sock.incoming() {
            match res {
                Ok(stream) => {
                    let server_ref = Arc::clone(&self);
                    std::thread::spawn(move || {
                        if let Err(_err) = server_ref.process(stream) {}
                    });
                }
                Err(e) => { return Err(ProxyError::SocketListenError); }
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
