use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

pub async fn recv(stream: &mut TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0; 128];
    let mut data = Vec::new();
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => return Ok(data),
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                if n < buf.len() {
                    return Ok(data);
                }
            }
            Err(e) => return Err(anyhow::anyhow!("net: error reading from stream: {}", e)),
        }
    }
}
