//! macOS: a tiny DNS responder for `*.test` inside Hearth, and `/etc/resolver/test` pointing macOS
//! at it. Only `.test` questions ever reach it (that is what the resolver file routes).

use std::net::SocketAddr;

/// Where the responder listens.
pub const PORT: u16 = 10053;
pub const RESOLVER_FILE: &str = "/etc/resolver/test";

const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;
const TTL: u32 = 60;

/// The text of `/etc/resolver/test`.
pub fn resolver_file_text(port: u16) -> String {
    format!("# Hearth: .test names\nnameserver 127.0.0.1\nport {port}\n")
}

/// The question's name (lowercase, dot-joined) and the offset just past its type and class.
fn question(query: &[u8]) -> Option<(String, u16, usize)> {
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let len = *query.get(at)? as usize;
        at += 1;
        if len == 0 {
            break;
        }
        // Compression pointers never appear in a question we accept.
        if len > 63 {
            return None;
        }
        let label = query.get(at..at + len)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        at += len;
    }
    let qtype = u16::from_be_bytes([*query.get(at)?, *query.get(at + 1)?]);
    let qclass = u16::from_be_bytes([*query.get(at + 2)?, *query.get(at + 3)?]);
    (qclass == CLASS_IN).then(|| (labels.join("."), qtype, at + 4))
}

/// The AppleScript that writes the resolver file (from the prepared copy at `prepared`) with the
/// user's admin password.
pub fn setup_script(prepared: &std::path::Path) -> String {
    let source = prepared.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"").replace('\'', "'\\\\''");
    format!(
        "do shell script \"mkdir -p /etc/resolver && cp '{source}' {RESOLVER_FILE} && chmod 644 {RESOLVER_FILE}\" with administrator privileges"
    )
}

/// Whether macOS sends `.test` questions to the responder.
pub fn check() -> Result<(), crate::CoreError> {
    if let Some(problem) = responder_problem() {
        return Err(crate::CoreError::Php(problem));
    }
    match std::fs::read_to_string(RESOLVER_FILE) {
        Ok(text) if text == resolver_file_text(PORT) => Ok(()),
        _ => Err(crate::CoreError::Php(format!("{RESOLVER_FILE} is not set up"))),
    }
}

/// Writes the resolver file; macOS asks for the admin password.
#[cfg(target_os = "macos")]
pub fn install() -> Result<(), crate::CoreError> {
    let prepared = std::env::temp_dir().join("hearth-resolver-test");
    std::fs::write(&prepared, resolver_file_text(PORT)).map_err(|e| crate::CoreError::io(&prepared, e))?;
    let output = std::process::Command::new("osascript")
        .args(["-e", &setup_script(&prepared)])
        .output()
        .map_err(|e| crate::CoreError::Php(format!("Could not run osascript: {e}")))?;
    let _ = std::fs::remove_file(&prepared);
    if output.status.success() {
        Ok(())
    } else {
        Err(crate::CoreError::Php("Setting up .test names was not confirmed".into()))
    }
}

/// The answer to one DNS query packet, or `None` for a packet that is not a usable query.
pub fn answer(query: &[u8]) -> Option<Vec<u8>> {
    let header = query.get(..12)?;
    let is_query = header[2] & 0x80 == 0;
    let one_question = header[4..6] == [0, 1];
    if !is_query || !one_question {
        return None;
    }
    let (name, qtype, end) = question(query)?;
    let ours = name.ends_with(".test") && name.len() > 5;
    let address: Option<Vec<u8>> = match (ours, qtype) {
        (true, TYPE_A) => Some(vec![127, 0, 0, 1]),
        (true, TYPE_AAAA) => Some(std::net::Ipv6Addr::LOCALHOST.octets().to_vec()),
        _ => None,
    };
    let mut out = Vec::with_capacity(end + 32);
    out.extend_from_slice(&header[..2]);
    // Response, same opcode and RD bit, authoritative; NXDOMAIN for names that are not ours.
    out.push(0x80 | (header[2] & 0x79) | 0x04);
    out.push(if ours { 0 } else { 3 });
    out.extend_from_slice(&[0, 1, 0, u8::from(address.is_some()), 0, 0, 0, 0]);
    out.extend_from_slice(&query[12..end]);
    if let Some(rdata) = address {
        out.extend_from_slice(&[0xc0, 0x0c]);
        out.extend_from_slice(&qtype.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out.extend_from_slice(&TTL.to_be_bytes());
        out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        out.extend_from_slice(&rdata);
    }
    Some(out)
}

/// Why the responder is not answering, when it could not start.
static PROBLEM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn responder_problem() -> Option<String> {
    PROBLEM.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Serves `answer` over UDP and TCP on `addr` until the runtime stops. A failure to listen is
/// kept for `check`, so the status never says ready while nothing answers.
pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let bound = async {
        Ok::<_, std::io::Error>((tokio::net::UdpSocket::bind(addr).await?, tokio::net::TcpListener::bind(addr).await?))
    };
    let (udp, tcp) = match bound.await {
        Ok(sockets) => {
            *PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) = None;
            sockets
        }
        Err(e) => {
            *PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(format!("The .test responder could not listen on port {}: {e}", addr.port()));
            return Err(e);
        }
    };
    serve_on(udp, tcp).await
}

async fn serve_on(udp: tokio::net::UdpSocket, tcp: tokio::net::TcpListener) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = tcp.accept().await {
            tokio::spawn(async move {
                // Length-prefixed messages; a client that stalls is dropped after 5 s.
                let read = async {
                    let len = stream.read_u16().await.ok()? as usize;
                    let mut query = vec![0u8; len];
                    stream.read_exact(&mut query).await.ok()?;
                    Some(query)
                };
                let Ok(Some(query)) = tokio::time::timeout(std::time::Duration::from_secs(5), read).await else {
                    return;
                };
                if let Some(reply) = answer(&query) {
                    let _ = stream.write_all(&(reply.len() as u16).to_be_bytes()).await;
                    let _ = stream.write_all(&reply).await;
                }
            });
        }
    });
    let mut buf = [0u8; 512];
    loop {
        let Ok((n, from)) = udp.recv_from(&mut buf).await else { continue };
        if let Some(reply) = answer(&buf[..n]) {
            let _ = udp.send_to(&reply, from).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The responder tests share its problem slot; one at a time.
    static SERVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q
    }

    fn rcode(answer: &[u8]) -> u8 {
        answer[3] & 0x0f
    }

    fn answer_count(answer: &[u8]) -> u16 {
        u16::from_be_bytes([answer[6], answer[7]])
    }

    #[test]
    fn a_test_name_gets_the_loopback_addresses() {
        let a = answer(&query("Shop.test", TYPE_A)).unwrap();
        assert_eq!(&a[..2], &[0x12, 0x34], "same id");
        assert_eq!(a[2] & 0x80, 0x80, "a response");
        assert_eq!((rcode(&a), answer_count(&a)), (0, 1));
        assert_eq!(&a[a.len() - 4..], &[127, 0, 0, 1]);
        let aaaa = answer(&query("api.shop.test", TYPE_AAAA)).unwrap();
        assert_eq!(answer_count(&aaaa), 1);
        assert_eq!(&aaaa[aaaa.len() - 16..], &std::net::Ipv6Addr::LOCALHOST.octets());
    }

    #[test]
    fn other_names_do_not_exist_and_other_types_have_no_answer() {
        let a = answer(&query("example.com", TYPE_A)).unwrap();
        assert_eq!((rcode(&a), answer_count(&a)), (3, 0));
        let mx = answer(&query("shop.test", 15)).unwrap();
        assert_eq!((rcode(&mx), answer_count(&mx)), (0, 0));
    }

    #[test]
    fn garbage_is_ignored() {
        assert_eq!(answer(&[1, 2, 3]), None);
        let mut truncated = query("shop.test", TYPE_A);
        truncated.truncate(truncated.len() - 3);
        assert_eq!(answer(&truncated), None);
        let mut response = query("shop.test", TYPE_A);
        response[2] |= 0x80;
        assert_eq!(answer(&response), None, "never answer a response");
    }

    #[test]
    fn the_resolver_file_points_at_the_responder() {
        assert_eq!(resolver_file_text(10053), "# Hearth: .test names\nnameserver 127.0.0.1\nport 10053\n");
    }

    #[tokio::test]
    async fn a_responder_that_cannot_listen_is_reported() {
        let _one = SERVE.lock().await;
        let taken = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap();
        assert!(serve(addr).await.is_err());
        let problem = responder_problem().unwrap_or_default();
        assert!(
            problem.starts_with(&format!("The .test responder could not listen on port {}", addr.port())),
            "{problem}"
        );
        assert_eq!(check().unwrap_err().to_string(), problem);
    }

    #[test]
    fn the_setup_script_copies_the_prepared_file_as_admin() {
        assert_eq!(
            setup_script(std::path::Path::new("/var/folders/x/T/hearth-resolver-test")),
            "do shell script \"mkdir -p /etc/resolver && cp '/var/folders/x/T/hearth-resolver-test' /etc/resolver/test && chmod 644 /etc/resolver/test\" with administrator privileges"
        );
    }

    #[tokio::test]
    async fn the_responder_answers_over_udp_and_tcp_and_survives_garbage() {
        let _one = SERVE.lock().await;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        tokio::spawn(serve(addr));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udp.send_to(&[9, 9, 9], addr).await.unwrap();
        udp.send_to(&query("shop.test", TYPE_A), addr).await.unwrap();
        let mut buf = [0u8; 512];
        let (n, _) =
            tokio::time::timeout(std::time::Duration::from_secs(2), udp.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf[n - 4..n], &[127, 0, 0, 1]);

        let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let q = query("shop.test", TYPE_AAAA);
        tcp.write_all(&(q.len() as u16).to_be_bytes()).await.unwrap();
        tcp.write_all(&q).await.unwrap();
        let len = tcp.read_u16().await.unwrap() as usize;
        let mut reply = vec![0u8; len];
        tcp.read_exact(&mut reply).await.unwrap();
        assert_eq!(answer_count(&reply), 1);
    }
}
