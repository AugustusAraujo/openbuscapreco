//! Cliente admin: uma conexão TCP por request (JSON-lines).

use crate::admin::{AdminRequest, AdminResponse};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

pub fn request(port: u16, req: &AdminRequest) -> std::io::Result<AdminResponse> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let payload = serde_json::to_string(req)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(stream, "{payload}")?;
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    serde_json::from_str(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
