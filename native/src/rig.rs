use std::{
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
#[derive(Debug, Clone)]
pub struct Radio {
    pub mhz: f64,
    pub mode: String,
}
pub fn read_radio(address: SocketAddr) -> Result<Radio, String> {
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|err| {
            match err.kind() {
                ErrorKind::ConnectionRefused => format!(
                "Connection refused at {address}. Start Rigctl Server in SDR++ and check its port."
            ),
                ErrorKind::TimedOut => format!("Timed out connecting to SDR++ at {address}."),
                _ => format!("Could not connect to SDR++ at {address} ({err})."),
            }
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .and_then(|()| stream.write_all(b"f\nm\n"))
        .map_err(|err| {
            format!("Could not send the frequency and mode requests to SDR++ ({err}).")
        })?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 256];
    let mut expected = "frequency and mode";
    loop {
        let timeout_message = || format!("SDR++ connected but did not return {expected} in time.");
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(timeout_message)?;
        stream
            .set_read_timeout(Some(left))
            .map_err(|err| format!("Could not configure the SDR++ read timeout ({err})."))?;
        let n = stream.read(&mut chunk).map_err(|err| match err.kind() {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => timeout_message(),
            _ => format!("Could not read {expected} from SDR++ ({err})."),
        })?;
        if n == 0 {
            return Err(format!(
                "SDR++ closed the connection before returning {expected}."
            ));
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > 8192 {
            return Err("SDR++ reply is too large.".into());
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "SDR++ sent a reply that is not valid UTF-8.")?;
        let lines: Vec<_> = text
            .split('\n')
            .take(text.bytes().filter(|b| *b == b'\n').count())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if let Some((index, reply)) = lines
            .iter()
            .take(2)
            .enumerate()
            .find(|(_, line)| line.starts_with("RPRT"))
        {
            let field = if index == 0 { "frequency" } else { "mode" };
            return Err(format!("SDR++ rejected the {field} request ({reply})."));
        }
        if lines.len() == 1 {
            expected = "the mode";
        }
        if lines.len() >= 2 {
            let hz = lines[0]
                .parse::<f64>()
                .map_err(|_| "Invalid SDR++ frequency.")?;
            if !hz.is_finite() || hz <= 0.0 {
                return Err("Invalid SDR++ frequency.".into());
            }
            if !lines[1]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err("Invalid SDR++ mode.".into());
            }
            return Ok(Radio {
                mhz: hz / 1e6,
                mode: lines[1].into(),
            });
        }
    }
}
