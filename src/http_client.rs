//! Shared HTTP client configuration and bounded response helpers.

use std::io::{self, Read, Write};
use std::sync::OnceLock;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Return a shared HTTP agent with consistent timeouts.
pub(crate) fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .timeout_write(WRITE_TIMEOUT)
            .build()
    })
}

/// Read a response into memory, enforcing a maximum byte size.
pub(crate) fn read_response_bytes(
    response: ureq::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, io::Error> {
    check_content_length(&response, max_bytes)?;
    let mut reader = response.into_reader();
    let mut limited = reader.take(max_bytes as u64 + 1);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response exceeded {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

/// Stream a response to the provided writer, enforcing a maximum byte size.
pub(crate) fn copy_response_to_writer(
    response: ureq::Response,
    writer: &mut impl Write,
    max_bytes: usize,
) -> Result<(), io::Error> {
    check_content_length(&response, max_bytes)?;
    let mut reader = response.into_reader();
    let mut limited = reader.take(max_bytes as u64 + 1);
    let mut total = 0usize;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = limited.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total += read;
        if total > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Response exceeded {max_bytes} bytes"),
            ));
        }
        writer.write_all(&buf[..read])?;
    }
    Ok(())
}

fn check_content_length(response: &ureq::Response, max_bytes: usize) -> Result<(), io::Error> {
    let Some(length) = response.header("Content-Length") else {
        return Ok(());
    };
    let Ok(length) = length.parse::<u64>() else {
        return Ok(());
    };
    if length > max_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Response too large: {length} bytes"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn read_response_bytes_rejects_content_length_over_max() {
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 100\r\n",
            "\r\n",
            "ok"
        )
        .to_string();
        let url = serve_once(response);
        let response = agent().get(&url).call().unwrap();
        let err = read_response_bytes(response, 10).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_response_bytes_rejects_body_over_max() {
        let body = "a".repeat(32);
        let response = format!("HTTP/1.0 200 OK\r\n\r\n{body}");
        let url = serve_once(response);
        let response = agent().get(&url).call().unwrap();
        let err = read_response_bytes(response, 16).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_response_bytes_accepts_under_limit() {
        let body = "hello";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let url = serve_once(response);
        let response = agent().get(&url).call().unwrap();
        let bytes = read_response_bytes(response, 16).unwrap();
        assert_eq!(bytes, body.as_bytes());
    }
}
