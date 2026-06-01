use log::{error, trace};
use std::io::{self, Write};
use tokio::io::AsyncReadExt;

/// Read one native-message frame from stdin.
///
/// Native messaging framing: 4-byte unsigned little-endian length prefix,
/// followed by `length` bytes of UTF-8 JSON payload.
pub async fn read_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<Option<Vec<u8>>, io::Error> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let length = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(payload))
}

/// Write one native-message frame to stdout.
pub fn write_message<W: Write>(writer: &mut W, json: &[u8]) -> io::Result<()> {
    let len = json.len();
    if len > u32::MAX as usize {
        error!("Message too large: {} bytes (max {})", len, u32::MAX);
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    let len_bytes = (len as u32).to_le_bytes();
    writer.write_all(&len_bytes)?;
    writer.write_all(json)?;
    writer.flush()?;
    Ok(())
}

/// Write a structured message to stdout synchronously.
pub fn send_message<W: Write>(writer: &mut W, msg: &impl serde::Serialize) -> io::Result<()> {
    let json =
        serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    trace!("→ sending {} bytes", json.len());
    write_message(writer, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip() {
        let payload = br#"{"type":"hello","browser":"firefox","extension_version":"0.1.0"}"#;

        // Encode
        let len = payload.len() as u32;
        let mut encoded = Vec::from(len.to_le_bytes());
        encoded.extend_from_slice(payload);

        // Decode
        let cursor = std::io::Cursor::new(encoded);
        let mut async_reader = tokio::io::BufReader::new(cursor);
        let result = read_message(&mut async_reader).await.unwrap().unwrap();
        assert_eq!(result, payload);
    }
}
