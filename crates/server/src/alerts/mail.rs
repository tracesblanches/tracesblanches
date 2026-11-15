//! E-mail out: a minimal SMTP submission client (RFC 5321, STARTTLS RFC 3207 or implicit
//! TLS, AUTH PLAIN RFC 4616) over rustls, and a recording mailer for tests.
//!
//! Meant for a submission relay with an account (Gmail `smtp.gmail.com:587` with an app
//! password, as another contact form does on the same cluster): no port 25, no
//! DKIM, no queue here (the outbox in SQLite retries).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;

use crate::util::{base64, random_hex};

/// A plain-text message to one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    /// Recipient address.
    pub to: String,
    /// Subject (UTF-8).
    pub subject: String,
    /// Body (UTF-8, `\n` line ends).
    pub body: String,
}

/// Something that delivers e-mail.
pub trait Mailer: Send + Sync {
    /// Sends one message; an error means "retry later".
    fn send(&self, mail: &Email) -> Result<()>;
    /// For logs and `/healthz`, without secrets.
    fn describe(&self) -> String;
}

/// Transport security.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    /// Plain connection upgraded by `STARTTLS` (port 587).
    StartTls,
    /// TLS from the start (port 465).
    Implicit,
    /// No TLS: loopback test servers only.
    None,
}

/// SMTP settings.
#[derive(Clone)]
pub struct SmtpConfig {
    /// Relay host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Security.
    pub tls: Tls,
    /// Account, if the relay wants one.
    pub user: Option<String>,
    /// Password (app password).
    pub pass: Option<String>,
    /// Sender address (Gmail forces the account's).
    pub from: String,
    /// Sender display name.
    pub from_name: String,
    /// Name given in `EHLO`.
    pub helo: String,
    /// Connect and read/write timeout.
    pub timeout: Duration,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("user", &self.user)
            .field("pass", &self.pass.as_ref().map(|_| "***"))
            .field("from", &self.from)
            .finish()
    }
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// TLS client configuration: rustls with ring and the Mozilla roots, as the pipeline's
/// downloader (no platform verifier, no aws-lc).
pub fn tls_config() -> Result<Arc<rustls::ClientConfig>> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| anyhow!("TLS setup: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// An SMTP session.
struct Session {
    stream: Option<Stream>,
    host: String,
    tls: Arc<rustls::ClientConfig>,
}

impl Session {
    fn s(&mut self) -> &mut Stream {
        self.stream.as_mut().expect("stream present")
    }

    /// Reads one reply: `(code, lines)`.
    fn reply(&mut self) -> Result<(u16, Vec<String>)> {
        let mut lines = Vec::new();
        loop {
            let mut line = Vec::new();
            let mut b = [0u8; 1];
            loop {
                let n = self.s().read(&mut b).context("reading the SMTP reply")?;
                if n == 0 {
                    bail!("SMTP server closed the connection");
                }
                if b[0] == b'\n' {
                    break;
                }
                line.push(b[0]);
                if line.len() > 4096 {
                    bail!("SMTP reply line too long");
                }
            }
            let text = String::from_utf8_lossy(&line).trim_end().to_owned();
            if text.len() < 3 {
                bail!("bad SMTP reply {text:?}");
            }
            let code: u16 = text[..3]
                .parse()
                .map_err(|_| anyhow!("bad SMTP reply {text:?}"))?;
            let last = text.as_bytes().get(3) != Some(&b'-');
            lines.push(text.get(4..).unwrap_or("").to_owned());
            if last {
                return Ok((code, lines));
            }
        }
    }

    fn expect(&mut self, ok: &[u16], what: &str) -> Result<Vec<String>> {
        let (code, lines) = self.reply()?;
        if ok.contains(&code) {
            Ok(lines)
        } else {
            bail!("SMTP {what}: {code} {}", lines.join(" / "))
        }
    }

    fn cmd(&mut self, line: &str, ok: &[u16], what: &str) -> Result<Vec<String>> {
        self.s().write_all(line.as_bytes())?;
        self.s().write_all(b"\r\n")?;
        self.s().flush()?;
        self.expect(ok, what)
    }

    fn start_tls(&mut self) -> Result<()> {
        let Some(Stream::Plain(tcp)) = self.stream.take() else {
            bail!("TLS already started");
        };
        let name = rustls::pki_types::ServerName::try_from(self.host.clone())
            .map_err(|e| anyhow!("server name {}: {e}", self.host))?;
        let conn = rustls::ClientConnection::new(Arc::clone(&self.tls), name)
            .map_err(|e| anyhow!("TLS: {e}"))?;
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        // Drive the handshake now, so that certificate errors come out here.
        while tls.conn.is_handshaking() {
            tls.conn
                .complete_io(&mut tls.sock)
                .context("TLS handshake")?;
        }
        self.stream = Some(Stream::Tls(Box::new(tls)));
        Ok(())
    }
}

/// SMTP submission client.
pub struct SmtpMailer {
    cfg: SmtpConfig,
    tls: Arc<rustls::ClientConfig>,
}

impl SmtpMailer {
    /// A client for this relay.
    pub fn new(cfg: SmtpConfig) -> Result<Self> {
        Ok(Self {
            cfg,
            tls: tls_config()?,
        })
    }

    /// Connects, greets and secures the session; returns it with the `EHLO` extensions
    /// (after TLS).
    fn open(&self) -> Result<(Session, Vec<String>)> {
        let addrs: Vec<_> = (self.cfg.host.as_str(), self.cfg.port)
            .to_socket_addrs()
            .with_context(|| format!("resolving {}", self.cfg.host))?
            .collect();
        let mut last = None;
        let mut tcp = None;
        for a in &addrs {
            match TcpStream::connect_timeout(a, self.cfg.timeout) {
                Ok(s) => {
                    tcp = Some(s);
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let tcp = tcp.ok_or_else(|| {
            anyhow!(
                "connecting to {}:{}: {}",
                self.cfg.host,
                self.cfg.port,
                last.map_or_else(|| "no address".to_owned(), |e| e.to_string())
            )
        })?;
        tcp.set_read_timeout(Some(self.cfg.timeout))?;
        tcp.set_write_timeout(Some(self.cfg.timeout))?;
        let mut s = Session {
            stream: Some(Stream::Plain(tcp)),
            host: self.cfg.host.clone(),
            tls: Arc::clone(&self.tls),
        };
        if self.cfg.tls == Tls::Implicit {
            s.start_tls()?;
        }
        s.expect(&[220], "greeting")?;
        let ehlo = format!("EHLO {}", self.cfg.helo);
        let mut ext = s.cmd(&ehlo, &[250], "EHLO")?;
        if self.cfg.tls == Tls::StartTls {
            if !ext.iter().any(|l| l.eq_ignore_ascii_case("STARTTLS")) {
                bail!("SMTP server does not offer STARTTLS");
            }
            s.cmd("STARTTLS", &[220], "STARTTLS")?;
            s.start_tls()?;
            ext = s.cmd(&ehlo, &[250], "EHLO after STARTTLS")?;
        }
        Ok((s, ext))
    }

    /// Connects and secures a session without logging in or sending, returns the server's
    /// extensions (checks host, port, TLS and certificate).
    pub fn probe(&self) -> Result<Vec<String>> {
        let (mut s, ext) = self.open()?;
        let _ = s.cmd("QUIT", &[221], "QUIT");
        Ok(ext)
    }

    /// The message as sent after `DATA`, CRLF line ends, not yet dot-stuffed.
    pub fn render(&self, mail: &Email) -> String {
        let domain = self
            .cfg
            .from
            .rsplit_once('@')
            .map_or("localhost", |(_, d)| d);
        let body = base64(mail.body.replace('\n', "\r\n").as_bytes());
        let mut wrapped = String::new();
        for chunk in body.as_bytes().chunks(76) {
            wrapped.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
            wrapped.push_str("\r\n");
        }
        format!(
            "From: {} <{}>\r\nTo: <{}>\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@{domain}>\r\n\
             MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: base64\r\nAuto-Submitted: auto-generated\r\n\
             X-Auto-Response-Suppress: All\r\n\r\n{wrapped}",
            encode_word(&self.cfg.from_name),
            self.cfg.from,
            mail.to,
            encode_word(&mail.subject),
            Utc::now().to_rfc2822(),
            random_hex(16).unwrap_or_else(|_| "0".repeat(32)),
        )
    }
}

/// RFC 2047 encoded word for a header value with non-ASCII characters.
fn encode_word(s: &str) -> String {
    if s.is_ascii() && !s.contains(['\r', '\n']) {
        s.to_owned()
    } else {
        // Encoded words are at most 75 characters: one per 45 bytes of UTF-8 (whole chars).
        let mut words = Vec::new();
        let mut cur = String::new();
        for c in s.chars().filter(|c| *c != '\r' && *c != '\n') {
            if cur.len() + c.len_utf8() > 45 {
                words.push(format!("=?UTF-8?B?{}?=", base64(cur.as_bytes())));
                cur.clear();
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            words.push(format!("=?UTF-8?B?{}?=", base64(cur.as_bytes())));
        }
        words.join("\r\n ")
    }
}

impl Mailer for SmtpMailer {
    fn send(&self, mail: &Email) -> Result<()> {
        if !crate::util::valid_email(&mail.to) {
            bail!("bad recipient address");
        }
        let (mut s, ext) = self.open()?;
        if let (Some(u), Some(p)) = (&self.cfg.user, &self.cfg.pass) {
            if !ext.iter().any(|l| {
                l.to_ascii_uppercase().starts_with("AUTH")
                    && l.to_ascii_uppercase().contains("PLAIN")
            }) {
                bail!("SMTP server does not offer AUTH PLAIN");
            }
            let token = base64(format!("\0{u}\0{p}").as_bytes());
            s.cmd(&format!("AUTH PLAIN {token}"), &[235], "AUTH")?;
        }
        s.cmd(
            &format!("MAIL FROM:<{}>", self.cfg.from),
            &[250],
            "MAIL FROM",
        )?;
        s.cmd(&format!("RCPT TO:<{}>", mail.to), &[250, 251], "RCPT TO")?;
        s.cmd("DATA", &[354], "DATA")?;
        let mut data = String::new();
        for line in self.render(mail).split_inclusive("\r\n") {
            if line.starts_with('.') {
                data.push('.');
            }
            data.push_str(line);
        }
        data.push_str(".\r\n");
        s.s().write_all(data.as_bytes())?;
        s.s().flush()?;
        s.expect(&[250], "end of DATA")?;
        let _ = s.cmd("QUIT", &[221], "QUIT");
        Ok(())
    }

    fn describe(&self) -> String {
        format!(
            "SMTP {}:{} ({:?}), from {}",
            self.cfg.host, self.cfg.port, self.cfg.tls, self.cfg.from
        )
    }
}

/// Keeps messages instead of sending them (tests, and when no relay is configured: the
/// outbox then waits until one is).
#[derive(Default)]
pub struct RecordingMailer {
    sent: Mutex<Vec<Email>>,
    fail: Mutex<u32>,
}

impl RecordingMailer {
    /// Messages "sent" so far.
    pub fn sent(&self) -> Vec<Email> {
        self.sent.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Makes the next `n` sends fail.
    pub fn fail_next(&self, n: u32) {
        *self.fail.lock().unwrap_or_else(|p| p.into_inner()) = n;
    }
}

impl Mailer for RecordingMailer {
    fn send(&self, mail: &Email) -> Result<()> {
        let mut f = self.fail.lock().unwrap_or_else(|p| p.into_inner());
        if *f > 0 {
            *f -= 1;
            bail!("simulated failure");
        }
        self.sent
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(mail.clone());
        Ok(())
    }

    fn describe(&self) -> String {
        "recording (test)".into()
    }
}

/// No relay configured: every send fails, the outbox keeps the messages.
pub struct NoMailer;

impl Mailer for NoMailer {
    fn send(&self, _mail: &Email) -> Result<()> {
        bail!("no SMTP relay configured (SMTP_HOST)")
    }
    fn describe(&self) -> String {
        "none (SMTP_HOST unset)".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_words() {
        assert_eq!(encode_word("skimap"), "skimap");
        assert_eq!(encode_word("é"), "=?UTF-8?B?w6k=?=");
        let long = encode_word(&"é".repeat(40));
        assert!(long.split("\r\n ").all(|w| w.len() <= 75), "{long}");
    }
}
