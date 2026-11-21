//! SMTP client against a fake relay on loopback (no TLS: `Tls::None` is only accepted for
//! loopback), and the whole alert chain through it. The STARTTLS handshake with a real
//! relay (Gmail) is the ignored test at the end.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::civil::date;
use jiff::tz::Offset;
use skimap_server::alerts::mail::{Email, Mailer, SmtpConfig, SmtpMailer, Tls};
use skimap_server::alerts::{Alerts, TripPlan};
use skimap_server::clock::{Clock, ManualClock};

/// What the fake relay saw: commands and messages.
#[derive(Default, Debug)]
struct Seen {
    commands: Vec<String>,
    messages: Vec<String>,
}

/// A fake submission relay: AUTH PLAIN must carry `user`/`pass`; `RCPT` to an address
/// starting with `reject` is refused.
fn fake_relay(user: &'static str, pass: &'static str) -> (u16, Arc<Mutex<Seen>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let c = c.unwrap();
            let seen = s2.clone();
            std::thread::spawn(move || session(c, user, pass, &seen));
        }
    });
    (port, seen)
}

fn session(c: TcpStream, user: &str, pass: &str, seen: &Mutex<Seen>) {
    let mut w = c.try_clone().unwrap();
    let mut r = BufReader::new(c);
    let say = |w: &mut TcpStream, s: &str| w.write_all(format!("{s}\r\n").as_bytes()).unwrap();
    say(&mut w, "220 fake ESMTP");
    let expected = skimap_server::util::base64(format!("\0{user}\0{pass}").as_bytes());
    let mut authed = false;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap() == 0 {
            return;
        }
        let line = line.trim_end().to_owned();
        seen.lock().unwrap().commands.push(line.clone());
        let up = line.to_ascii_uppercase();
        if up.starts_with("EHLO") {
            say(&mut w, "250-fake");
            say(&mut w, "250-AUTH LOGIN PLAIN");
            say(&mut w, "250 8BITMIME");
        } else if let Some(tok) = line.strip_prefix("AUTH PLAIN ") {
            if tok == expected {
                authed = true;
                say(&mut w, "235 ok");
            } else {
                say(&mut w, "535 bad credentials");
            }
        } else if up.starts_with("MAIL FROM") {
            say(
                &mut w,
                if authed {
                    "250 ok"
                } else {
                    "530 auth required"
                },
            );
        } else if up.starts_with("RCPT TO:<REJECT") {
            say(&mut w, "550 no such user");
        } else if up.starts_with("RCPT TO") {
            say(&mut w, "250 ok");
        } else if up == "DATA" {
            say(&mut w, "354 go");
            let mut msg = String::new();
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l == ".\r\n" {
                    break;
                }
                msg.push_str(&l);
            }
            seen.lock().unwrap().messages.push(msg);
            say(&mut w, "250 queued");
        } else if up == "QUIT" {
            say(&mut w, "221 bye");
            return;
        } else {
            say(&mut w, "502 ?");
        }
    }
}

fn mailer(port: u16, pass: &str) -> SmtpMailer {
    SmtpMailer::new(SmtpConfig {
        host: "127.0.0.1".into(),
        port,
        tls: Tls::None,
        user: Some("alertes@example.org".into()),
        pass: Some(pass.into()),
        from: "alertes@example.org".into(),
        from_name: "skimap (alertes)".into(),
        helo: "skimap.test".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap()
}

/// Body of a message: headers, then Base64 lines.
fn decode(msg: &str) -> (String, String) {
    let (head, body) = msg.split_once("\r\n\r\n").unwrap();
    let b64: String = body.lines().map(str::trim).collect();
    let bytes = decode_b64(&b64);
    (head.to_owned(), String::from_utf8(bytes).unwrap())
}

fn decode_b64(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut n = 0;
    for c in s.bytes().filter(|c| *c != b'=') {
        buf = (buf << 6) | A.iter().position(|a| *a == c).unwrap() as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((buf >> n) as u8);
        }
    }
    out
}

#[test]
fn sends_through_a_relay_with_auth() {
    let (port, seen) = fake_relay("alertes@example.org", "app-password");
    let m = mailer(port, "app-password");
    m.send(&Email {
        to: "paul@example.org".into(),
        subject: "Alerte skimap : Marie n'a pas confirmé son retour".into(),
        body: "Ligne 1\n.ligne qui commence par un point\nFin é".into(),
    })
    .unwrap();
    let seen = seen.lock().unwrap();
    let cmds: Vec<&str> = seen
        .commands
        .iter()
        .map(|c| c.split(' ').next().unwrap())
        .collect();
    assert_eq!(cmds, ["EHLO", "AUTH", "MAIL", "RCPT", "DATA", "QUIT"]);
    assert_eq!(seen.commands[2], "MAIL FROM:<alertes@example.org>");
    assert_eq!(seen.commands[3], "RCPT TO:<paul@example.org>");
    let (head, body) = decode(&seen.messages[0]);
    assert!(head.contains("To: <paul@example.org>"), "{head}");
    assert!(
        head.contains("From: skimap (alertes) <alertes@example.org>"),
        "{head}"
    );
    assert!(head.contains("Subject: =?UTF-8?B?"), "{head}");
    assert!(head.contains("Auto-Submitted: auto-generated"));
    assert_eq!(body, "Ligne 1\r\n.ligne qui commence par un point\r\nFin é");
}

#[test]
fn relay_errors_are_errors() {
    let (port, _) = fake_relay("alertes@example.org", "app-password");
    let e = mailer(port, "wrong")
        .send(&Email {
            to: "paul@example.org".into(),
            subject: "s".into(),
            body: "b".into(),
        })
        .unwrap_err();
    assert!(format!("{e:#}").contains("535"), "{e:#}");
    let e = mailer(port, "app-password")
        .send(&Email {
            to: "reject@example.org".into(),
            subject: "s".into(),
            body: "b".into(),
        })
        .unwrap_err();
    assert!(format!("{e:#}").contains("550"), "{e:#}");
    // Nobody listening.
    let free = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert!(
        mailer(free, "x")
            .send(&Email {
                to: "a@b.fr".into(),
                subject: "s".into(),
                body: "b".into()
            })
            .is_err()
    );
}

#[test]
fn whole_alert_chain_through_smtp() {
    let (port, seen) = fake_relay("alertes@example.org", "app-password");
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(
        skimap_common::dates::utc(date(2027, 1, 10).at(7, 0, 0, 0)).unwrap(),
    ));
    let alerts = Alerts::open(
        &dir.path().join("a.sqlite"),
        Arc::new(mailer(port, "app-password")),
        clock.clone(),
        "https://skimap.test",
    )
    .unwrap();
    let plan: TripPlan = serde_json::from_value(serde_json::json!({
        "skier_name": "Marie", "contacts": ["paul@example.org"],
        "deadline": (clock.now() + jiff::SignedDuration::from_hours(8)).display_with_offset(Offset::UTC).to_string(),
        "grace_minutes": 0, "site": "chapelle-des-bois",
        "plan": "Crêtes du Risoux, aller-retour."
    }))
    .unwrap();
    alerts.create(plan).unwrap().unwrap();
    clock.advance(jiff::SignedDuration::from_hours(8));
    let rep = alerts.tick().unwrap();
    assert_eq!((rep.alerted, rep.sent), (1, 1));
    let seen = seen.lock().unwrap();
    let (_, body) = decode(&seen.messages[0]);
    assert!(
        body.contains("dimanche 10/01/2027 à 16:00 (heure de Paris)"),
        "{body}"
    );
    assert!(body.contains("Secteur : Chapelle-des-Bois (25)"), "{body}");
    assert!(body.contains("Crêtes du Risoux"));
}

/// STARTTLS with Gmail's submission relay, up to the second EHLO (no login, nothing
/// sent): checks the TLS stack and the certificate chain on a real relay.
#[test]
#[ignore = "network: smtp.gmail.com:587"]
fn starttls_with_gmail() {
    let m = SmtpMailer::new(SmtpConfig {
        host: "smtp.gmail.com".into(),
        port: 587,
        tls: Tls::StartTls,
        user: None,
        pass: None,
        from: "probe@example.org".into(),
        from_name: "probe".into(),
        helo: "skimap.warnier.net".into(),
        timeout: Duration::from_secs(20),
    })
    .unwrap();
    let ext = m.probe().unwrap();
    println!("EHLO after STARTTLS: {ext:?}");
    assert!(
        ext.iter()
            .any(|l| l.starts_with("AUTH") && l.contains("PLAIN"))
    );
}
