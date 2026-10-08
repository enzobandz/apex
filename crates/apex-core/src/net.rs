//! Network measurements: DNS resolver response time and TCP connect latency.
//!
//! DNS queries are built by hand (RFC 1035) and sent directly to each resolver over
//! UDP, so different resolvers can be compared without changing system settings.
//! ICMP ping needs raw sockets or a Windows-specific API; TCP connect time to port
//! 443 is used as a portable latency/jitter/loss probe and is labelled as such.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use crate::bench::stats;

pub fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // standard query, recursion desired
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // QDCOUNT=1
    for label in name.trim_end_matches('.').split('.') {
        let l = label.as_bytes();
        q.push(l.len().min(63) as u8);
        q.extend_from_slice(&l[..l.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0, 1]); // QTYPE=A, QCLASS=IN
    q
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedResponse {
    pub id: u16,
    pub rcode: u8,
    pub answers: u16,
}

pub fn parse_response(buf: &[u8]) -> Option<ParsedResponse> {
    if buf.len() < 12 || buf[2] & 0x80 == 0 {
        return None; // too short or not a response
    }
    Some(ParsedResponse {
        id: u16::from_be_bytes([buf[0], buf[1]]),
        rcode: buf[3] & 0x0F,
        answers: u16::from_be_bytes([buf[6], buf[7]]),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyResult {
    pub target: String,
    pub label: String,
    pub method: String,
    pub sent: u32,
    pub received: u32,
    pub loss_percent: f64,
    pub samples_ms: Vec<f64>,
    pub mean_ms: Option<f64>,
    pub median_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mean absolute difference between consecutive samples (RFC 3550 style jitter).
    pub jitter_ms: Option<f64>,
    pub errors: Vec<String>,
}

fn summarize(
    target: String,
    label: String,
    method: &str,
    sent: u32,
    samples: Vec<f64>,
    errors: Vec<String>,
) -> LatencyResult {
    let st = stats(&samples);
    let jitter = if samples.len() >= 2 {
        Some(
            samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>()
                / (samples.len() - 1) as f64,
        )
    } else {
        None
    };
    LatencyResult {
        target,
        label,
        method: method.into(),
        sent,
        received: samples.len() as u32,
        loss_percent: if sent == 0 {
            0.0
        } else {
            (sent - samples.len() as u32) as f64 / sent as f64 * 100.0
        },
        mean_ms: st.as_ref().map(|s| s.mean),
        median_ms: st.as_ref().map(|s| s.median),
        min_ms: st.as_ref().map(|s| s.min),
        max_ms: st.as_ref().map(|s| s.max),
        jitter_ms: jitter,
        samples_ms: samples,
        errors,
    }
}

/// Query `resolver` for each name in `names`, `rounds` times. The first round is
/// reported separately in errors if it fails but is included in timing, because
/// uncached lookups are part of real-world experience.
pub fn dns_test(
    resolver: IpAddr,
    label: &str,
    names: &[&str],
    rounds: u32,
    timeout: Duration,
) -> LatencyResult {
    dns_test_at(SocketAddr::new(resolver, 53), label, names, rounds, timeout)
}

pub fn dns_test_at(
    target: SocketAddr,
    label: &str,
    names: &[&str],
    rounds: u32,
    timeout: Duration,
) -> LatencyResult {
    let resolver = target.ip();
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    let mut sent = 0;
    let bind: SocketAddr = if resolver.is_ipv4() {
        "0.0.0.0:0".parse().unwrap()
    } else {
        "[::]:0".parse().unwrap()
    };
    let sock = match UdpSocket::bind(bind) {
        Ok(s) => s,
        Err(e) => {
            return summarize(
                resolver.to_string(),
                label.into(),
                "DNS over UDP/53",
                0,
                vec![],
                vec![e.to_string()],
            )
        }
    };
    let _ = sock.set_read_timeout(Some(timeout));
    let mut id: u16 = (std::process::id() as u16).wrapping_mul(31);
    for _ in 0..rounds {
        for name in names {
            id = id.wrapping_add(1);
            sent += 1;
            let q = build_query(id, name);
            let t = Instant::now();
            if let Err(e) = sock.send_to(&q, target) {
                errors.push(format!("{name}: {e}"));
                continue;
            }
            let mut buf = [0u8; 1500];
            loop {
                match sock.recv_from(&mut buf) {
                    Ok((n, from)) if from.ip() == resolver => match parse_response(&buf[..n]) {
                        Some(r) if r.id == id => {
                            if r.rcode == 0 || r.rcode == 3 {
                                samples.push(t.elapsed().as_secs_f64() * 1000.0);
                            } else {
                                errors.push(format!(
                                    "{name}: resolver returned error code {}",
                                    r.rcode
                                ));
                            }
                            break;
                        }
                        _ => continue, // stale reply from an earlier timed-out query
                    },
                    Ok(_) => continue,
                    Err(_) => {
                        errors.push(format!(
                            "{name}: no reply within {} ms",
                            timeout.as_millis()
                        ));
                        break;
                    }
                }
            }
        }
    }
    summarize(
        resolver.to_string(),
        label.into(),
        "DNS over UDP/53",
        sent,
        samples,
        errors,
    )
}

/// TCP handshake time to `addr` (e.g. 1.1.1.1:443), `count` times.
pub fn tcp_latency(addr: SocketAddr, label: &str, count: u32, timeout: Duration) -> LatencyResult {
    let mut samples = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..count {
        let t = Instant::now();
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                samples.push(t.elapsed().as_secs_f64() * 1000.0);
                drop(s);
            }
            Err(e) => errors.push(e.to_string()),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    summarize(
        addr.to_string(),
        label.into(),
        "TCP connect",
        count,
        samples,
        errors,
    )
}

/// Public resolvers offered for comparison. Users can add their own.
pub const KNOWN_RESOLVERS: &[(&str, &str)] = &[
    ("1.1.1.1", "Cloudflare"),
    ("8.8.8.8", "Google Public DNS"),
    ("9.9.9.9", "Quad9"),
    ("208.67.222.222", "OpenDNS"),
];

pub const TEST_NAMES: &[&str] = &[
    "microsoft.com",
    "steampowered.com",
    "wikipedia.org",
    "github.com",
    "youtube.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding() {
        let q = build_query(0x1234, "example.com");
        assert_eq!(&q[..2], &[0x12, 0x34]);
        assert_eq!(&q[12..20], &[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e']);
        assert_eq!(&q[q.len() - 5..], &[0, 0, 1, 0, 1]);
    }

    #[test]
    fn response_parsing() {
        let mut r = build_query(7, "a.b");
        r[2] |= 0x80; // QR=1
        r[3] = 0x03; // NXDOMAIN
        r[7] = 2;
        assert_eq!(
            parse_response(&r),
            Some(ParsedResponse {
                id: 7,
                rcode: 3,
                answers: 2
            })
        );
        assert_eq!(parse_response(&build_query(7, "a.b")), None);
    }

    #[test]
    fn loss_and_jitter_math() {
        let r = summarize(
            "x".into(),
            "x".into(),
            "t",
            4,
            vec![10.0, 14.0, 12.0],
            vec![],
        );
        assert_eq!(r.received, 3);
        assert!((r.loss_percent - 25.0).abs() < 1e-9);
        assert!((r.jitter_ms.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn dns_test_against_local_responder() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            for i in 0..4 {
                if let Ok((n, from)) = server.recv_from(&mut buf) {
                    if i == 1 {
                        continue; // drop one query to exercise loss accounting
                    }
                    buf[2] |= 0x80;
                    let _ = server.send_to(&buf[..n], from);
                }
            }
        });
        let r = dns_test_at(
            addr,
            "local",
            &["a.test", "b.test"],
            2,
            Duration::from_millis(300),
        );
        assert_eq!(r.sent, 4);
        assert_eq!(r.received, 3);
        assert!((r.loss_percent - 25.0).abs() < 1e-9);
        assert_eq!(r.errors.len(), 1);
    }
}
