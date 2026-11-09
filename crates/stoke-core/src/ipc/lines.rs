use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use super::protocol::MAX_LINE;

/// Reads one `\n`-terminated line without the terminator. `None` at a clean end of stream; an
/// `InvalidData` error for a line longer than `MAX_LINE`.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        let (chunk, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (&buf[..i], i + 1),
            None => (buf, buf.len()),
        };
        let found = done > chunk.len();
        line.extend_from_slice(chunk);
        reader.consume(done);
        if line.len() > MAX_LINE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
        if found {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
    }
}
