use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::mail::smtp::{self, Catcher};
use crate::mail::store;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

/// A catcher on a free port that takes mails up to `max` bytes.
async fn catcher(max: usize) -> (tempfile::TempDir, HearthPaths, u16) {
    let (d, paths) = home();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(smtp::serve_limited(listener, paths.clone(), max));
    (d, paths, port)
}

struct Client {
    r: BufReader<OwnedReadHalf>,
    w: OwnedWriteHalf,
}

impl Client {
    async fn connect(port: u16) -> (Client, String) {
        let (r, w) = TcpStream::connect(("127.0.0.1", port)).await.unwrap().into_split();
        let mut c = Client { r: BufReader::new(r), w };
        let greeting = c.reply().await;
        (c, greeting)
    }

    /// One reply, all its lines (`250-...` then `250 ...`).
    async fn reply(&mut self) -> String {
        let mut all = String::new();
        loop {
            let mut line = String::new();
            self.r.read_line(&mut line).await.unwrap();
            all.push_str(&line);
            if line.len() < 4 || line.as_bytes()[3] != b'-' {
                return all;
            }
        }
    }

    async fn send(&mut self, line: &str) -> String {
        self.w.write_all(format!("{line}\r\n").as_bytes()).await.unwrap();
        self.reply().await
    }

    async fn raw(&mut self, bytes: &[u8]) {
        self.w.write_all(bytes).await.unwrap();
    }
}

fn code(reply: &str) -> &str {
    &reply[..3]
}

fn b64(s: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

#[tokio::test]
async fn lettre_sends_with_a_username_and_it_is_kept_for_that_site() {
    use lettre::transport::smtp::authentication::{Credentials, Mechanism};
    use lettre::{Message, SmtpTransport, Transport};
    let (_d, paths, port) = catcher(smtp::MAX_SIZE).await;
    tokio::task::spawn_blocking(move || {
        let mail = Message::builder()
            .from("Shop <shop@example.test>".parse().unwrap())
            .to("ann@example.test".parse().unwrap())
            .bcc("audit@example.test".parse().unwrap())
            .subject("Welcome")
            .body(String::from("Hello Ann"))
            .unwrap();
        let mailer = SmtpTransport::builder_dangerous("127.0.0.1")
            .port(port)
            .credentials(Credentials::new("shop".into(), "null".into()))
            .authentication(vec![Mechanism::Login])
            .build();
        mailer.send(&mail).unwrap();
    })
    .await
    .unwrap();
    let list = store::list(&paths);
    assert_eq!(list.len(), 1);
    let s = &list[0];
    assert_eq!((s.site.as_deref(), s.subject.as_str()), (Some("shop"), "Welcome"));
    assert_eq!(s.to, vec!["ann@example.test".to_string(), "audit@example.test".to_string()]);
}

#[tokio::test]
async fn plain_smtp_without_auth_is_kept_without_a_site() {
    let (_d, paths, port) = catcher(smtp::MAX_SIZE).await;
    let (mut c, greeting) = Client::connect(port).await;
    assert_eq!(code(&greeting), "220");
    let ehlo = c.send("EHLO client").await;
    assert!(ehlo.contains("250-SIZE 26214400") && ehlo.contains("AUTH PLAIN LOGIN"), "{ehlo}");
    assert_eq!(code(&c.send("MAIL FROM:<app@example.test> BODY=8BITMIME").await), "250");
    assert_eq!(code(&c.send("RCPT TO:<ann@example.test>").await), "250");
    assert_eq!(code(&c.send("rcpt to:<bob@example.test>").await), "250");
    assert_eq!(code(&c.send("DATA").await), "354");
    c.raw(b"Subject: Hi\r\n\r\n..starts with a dot\r\nbody\r\n.\r\n").await;
    let queued = c.reply().await;
    assert_eq!(code(&queued), "250", "{queued}");
    assert_eq!(code(&c.send("QUIT").await), "221");
    let s = &store::list(&paths)[0];
    assert_eq!((s.site.as_deref(), s.from.as_str(), s.subject.as_str()), (None, "app@example.test", "Hi"));
    assert_eq!(s.to.len(), 2);
    let (raw, _) = store::raw(&paths, &s.id).unwrap();
    assert_eq!(raw, "Subject: Hi\r\n\r\n.starts with a dot\r\nbody\r\n");
}

#[tokio::test]
async fn auth_plain_and_login_both_work() {
    async fn send_one(c: &mut Client) {
        assert_eq!(code(&c.send("MAIL FROM:<a@x.test>").await), "250");
        assert_eq!(code(&c.send("RCPT TO:<b@x.test>").await), "250");
        assert_eq!(code(&c.send("DATA").await), "354");
        assert_eq!(code(&c.send("Subject: s\r\n\r\nx\r\n.").await), "250");
    }
    let (_d, paths, port) = catcher(smtp::MAX_SIZE).await;
    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(code(&c.send(&format!("AUTH PLAIN {}", b64(b"\0blog\0pw"))).await), "235");
    assert_eq!(code(&c.send("AUTH PLAIN").await), "503", "only one login per session");
    send_one(&mut c).await;

    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(c.send("AUTH PLAIN").await.trim_end(), "334");
    assert_eq!(code(&c.send(&b64(b"\0news\0pw")).await), "235");
    send_one(&mut c).await;

    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(c.send("AUTH LOGIN").await.trim_end(), "334 VXNlcm5hbWU6");
    assert_eq!(c.send(&b64(b" shop ")).await.trim_end(), "334 UGFzc3dvcmQ6");
    assert_eq!(code(&c.send(&b64(b"pw")).await), "235");
    send_one(&mut c).await;

    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(code(&c.send("AUTH CRAM-MD5").await), "504");
    assert_eq!(code(&c.send("AUTH PLAIN !!!").await), "501");

    let mut sites: Vec<_> = store::list(&paths).into_iter().map(|s| s.site.unwrap()).collect();
    sites.sort();
    assert_eq!(sites, ["blog", "news", "shop"]);
}

#[tokio::test]
async fn a_dropped_connection_mid_data_keeps_nothing() {
    let (_d, paths, port) = catcher(smtp::MAX_SIZE).await;
    let (mut c, _) = Client::connect(port).await;
    c.send("HELO c").await;
    c.send("MAIL FROM:<a@x.test>").await;
    c.send("RCPT TO:<b@x.test>").await;
    assert_eq!(code(&c.send("DATA").await), "354");
    c.raw(b"Subject: half\r\n\r\nsome of the body").await;
    drop(c);
    let (mut c, greeting) = Client::connect(port).await;
    assert_eq!(code(&greeting), "220", "the catcher still serves");
    assert_eq!(code(&c.send("NOOP").await), "250");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(store::list(&paths).is_empty());
}

#[tokio::test]
async fn too_big_is_552_and_the_connection_goes_on() {
    let (_d, paths, port) = catcher(1000).await;
    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(code(&c.send("MAIL FROM:<a@x.test> SIZE=5000").await), "552");
    assert_eq!(code(&c.send("MAIL FROM:<a@x.test>").await), "250");
    c.send("RCPT TO:<b@x.test>").await;
    assert_eq!(code(&c.send("DATA").await), "354");
    let big = format!("Subject: big\r\n\r\n{}\r\n.", "x".repeat(2000));
    assert_eq!(code(&c.send(&big).await), "552");
    assert_eq!(code(&c.send("MAIL FROM:<a@x.test>").await), "250", "a new mail may start");
    c.send("RCPT TO:<b@x.test>").await;
    c.send("DATA").await;
    assert_eq!(code(&c.send("Subject: small\r\n\r\nok\r\n.").await), "250");
    let list = store::list(&paths);
    assert_eq!(list.iter().map(|s| s.subject.as_str()).collect::<Vec<_>>(), ["small"]);
}

#[tokio::test]
async fn commands_out_of_order_get_503() {
    let (_d, paths, port) = catcher(smtp::MAX_SIZE).await;
    let (mut c, _) = Client::connect(port).await;
    c.send("EHLO c").await;
    assert_eq!(code(&c.send("RCPT TO:<b@x.test>").await), "503");
    assert_eq!(code(&c.send("DATA").await), "503");
    assert_eq!(code(&c.send("MAIL FROM:<a@x.test>").await), "250");
    assert_eq!(code(&c.send("MAIL FROM:<a@x.test>").await), "503");
    assert_eq!(code(&c.send("DATA").await), "503", "no recipient yet");
    assert_eq!(code(&c.send("RSET").await), "250");
    assert_eq!(code(&c.send("RCPT TO:<b@x.test>").await), "503", "RSET forgot the sender");
    assert_eq!(code(&c.send("VRFY ann").await), "252");
    assert_eq!(code(&c.send("STARTTLS").await), "502");
    assert_eq!(code(&c.send("FOO").await), "502");
    assert_eq!(code(&c.send(&format!("NOOP {}", "y".repeat(5000))).await), "500");
    assert_eq!(code(&c.send("NOOP").await), "250", "the long line was skipped whole");
    assert!(store::list(&paths).is_empty());
}

#[tokio::test]
async fn a_taken_port_is_reported_not_fatal() {
    let (_d, paths) = home();
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port();
    let catcher = Catcher::default();
    let err = catcher.start(&paths, taken).await.unwrap_err().to_string();
    assert!(err.contains(&format!("Port {taken} is taken")), "{err}");
    let status = catcher.status();
    assert_eq!((status.port, status.listening, status.error), (taken, false, Some(err)));

    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    catcher.start(&paths, free).await.unwrap();
    assert_eq!((catcher.status().port, catcher.status().listening, catcher.status().error), (free, true, None));
    let (_c, greeting) = Client::connect(free).await;
    assert_eq!(code(&greeting), "220");

    assert!(catcher.start(&paths, taken).await.is_err());
    let status = catcher.status();
    assert_eq!((status.port, status.listening, status.error), (free, true, None), "the old port keeps listening");

    catcher.stop();
    assert!(!catcher.status().listening);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(TcpStream::connect(("127.0.0.1", free)).await.is_err());
}

#[test]
fn the_port_is_kept_in_config() {
    let (_d, paths) = home();
    assert_eq!(smtp::configured_port(&paths), 2525);
    smtp::set_configured_port(&paths, 2626).unwrap();
    assert_eq!(smtp::configured_port(&paths), 2626);
    std::fs::write(paths.config.join("mail.json"), "garbage").unwrap();
    assert_eq!(smtp::configured_port(&paths), 2525);
}
