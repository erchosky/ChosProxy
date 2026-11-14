//! The minimal DNS needed to observe lookups (RFC 1035 wire format).
//!
//! The server is authoritative for one zone only: it answers `A` (and
//! `AAAA` when configured) for names inside the zone, returns an empty
//! answer for other types there, and refuses everything else. It never
//! recurses, never uses name compression in what it reads, and keeps
//! answers about as small as the question, so it cannot be used as an open
//! resolver or a reflection amplifier.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Longest packet read or written (classic UDP DNS).
pub const MAX_PACKET: usize = 512;
const HEADER: usize = 12;

/// Record types the server answers.
pub const TYPE_A: u16 = 1;
/// IPv6 address record.
pub const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;

/// Why a packet was not answered normally.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DnsError {
    /// Too short, truncated or malformed.
    #[error("malformed DNS packet")]
    Malformed,
    /// A response, or an opcode other than a standard query.
    #[error("not a standard query")]
    NotQuery,
}

/// A parsed question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Transaction id.
    pub id: u16,
    /// Recursion desired (echoed back).
    pub recursion_desired: bool,
    /// The name, lowercase, without the trailing dot.
    pub name: String,
    /// Record type.
    pub qtype: u16,
    /// Class.
    pub qclass: u16,
    /// The question section as received (echoed in the answer).
    question: Vec<u8>,
}

fn u16_at(p: &[u8], at: usize) -> Result<u16, DnsError> {
    match p.get(at..at + 2) {
        Some([a, b]) => Ok(u16::from_be_bytes([*a, *b])),
        _ => Err(DnsError::Malformed),
    }
}

/// Parses a query with exactly one question.
pub fn parse_query(p: &[u8]) -> Result<Query, DnsError> {
    if p.len() < HEADER || p.len() > MAX_PACKET {
        return Err(DnsError::Malformed);
    }
    let id = u16_at(p, 0)?;
    let flags = u16_at(p, 2)?;
    let is_response = flags & 0x8000 != 0;
    let opcode = (flags >> 11) & 0xf;
    if is_response || opcode != 0 {
        return Err(DnsError::NotQuery);
    }
    if u16_at(p, 4)? != 1 {
        return Err(DnsError::Malformed);
    }
    let mut at = HEADER;
    let mut labels: Vec<String> = Vec::new();
    let mut name_len = 0usize;
    loop {
        let len = usize::from(*p.get(at).ok_or(DnsError::Malformed)?);
        at += 1;
        if len == 0 {
            break;
        }
        // Compression pointers and extended label types are not valid in
        // a question the server needs to read.
        if len > 63 {
            return Err(DnsError::Malformed);
        }
        let label = p.get(at..at + len).ok_or(DnsError::Malformed)?;
        if !label
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
        {
            return Err(DnsError::Malformed);
        }
        name_len += len + 1;
        if name_len > 255 {
            return Err(DnsError::Malformed);
        }
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        at += len;
    }
    let qtype = u16_at(p, at)?;
    let qclass = u16_at(p, at + 2)?;
    at += 4;
    Ok(Query {
        id,
        recursion_desired: flags & 0x0100 != 0,
        name: labels.join("."),
        qtype,
        qclass,
        question: p[HEADER..at].to_vec(),
    })
}

/// Whether `name` is the zone or inside it.
pub fn in_zone(name: &str, zone: &str) -> bool {
    let zone = zone.trim_end_matches('.').to_ascii_lowercase();
    !zone.is_empty() && (name == zone || name.ends_with(&format!(".{zone}")))
}

/// Addresses the server gives for names in its zone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZoneAddresses {
    /// Answer for `A` queries.
    pub ipv4: Option<Ipv4Addr>,
    /// Answer for `AAAA` queries.
    pub ipv6: Option<Ipv6Addr>,
}

/// The response to `q`: an authoritative answer inside `zone`, REFUSED
/// outside it.
pub fn respond(q: &Query, zone: &str, addresses: ZoneAddresses) -> Vec<u8> {
    let inside = in_zone(&q.name, zone) && q.qclass == CLASS_IN;
    let rdata: Option<Vec<u8>> = if !inside {
        None
    } else {
        match q.qtype {
            TYPE_A => addresses.ipv4.map(|a| a.octets().to_vec()),
            TYPE_AAAA => addresses.ipv6.map(|a| a.octets().to_vec()),
            _ => None,
        }
    };
    // QR, AA when authoritative, RD echoed, RA never; RCODE 5 = REFUSED.
    let mut flags: u16 = 0x8000;
    if inside {
        flags |= 0x0400;
    } else {
        flags |= 5;
    }
    if q.recursion_desired {
        flags |= 0x0100;
    }
    let answers: u16 = u16::from(rdata.is_some());
    let mut out = Vec::with_capacity(HEADER + q.question.len() + 28);
    out.extend_from_slice(&q.id.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&answers.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&q.question);
    if let Some(rdata) = rdata {
        out.extend_from_slice(&[0xc0, 0x0c]); // pointer to the question name
        out.extend_from_slice(&q.qtype.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // TTL 0: every lookup reaches us
        let len = u16::try_from(rdata.len()).unwrap_or(0);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&rdata);
    }
    out
}

/// A query packet (tests and the self-check of the server).
pub fn build_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        out.push(u8::try_from(label.len()).unwrap_or(0));
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&CLASS_IN.to_be_bytes());
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    const ADDR: ZoneAddresses = ZoneAddresses {
        ipv4: Some(Ipv4Addr::new(203, 0, 113, 7)),
        ipv6: None,
    };

    #[test]
    fn answers_inside_the_zone_and_refuses_outside() {
        let q = parse_query(&build_query(0x1234, "AbC.Oast.Example", TYPE_A)).unwrap();
        assert_eq!(q.name, "abc.oast.example");
        let r = respond(&q, "oast.example", ADDR);
        assert_eq!(&r[0..2], &[0x12, 0x34]);
        assert_eq!(r[3] & 0x0f, 0, "NOERROR");
        assert_eq!(u16_at(&r, 6).unwrap(), 1, "one answer");
        assert_eq!(&r[r.len() - 4..], &[203, 0, 113, 7]);
        assert!(r.len() <= 2 * build_query(0, "abc.oast.example", TYPE_A).len());

        let other = parse_query(&build_query(1, "example.com", TYPE_A)).unwrap();
        let r = respond(&other, "oast.example", ADDR);
        assert_eq!(r[3] & 0x0f, 5, "REFUSED");
        assert_eq!(u16_at(&r, 6).unwrap(), 0);
        assert!(!in_zone("evil-oast.example", "oast.example"));

        // AAAA without an address: empty authoritative answer.
        let aaaa = parse_query(&build_query(2, "x.oast.example", TYPE_AAAA)).unwrap();
        let r = respond(&aaaa, "oast.example", ADDR);
        assert_eq!((r[3] & 0x0f, u16_at(&r, 6).unwrap()), (0, 0));
    }

    #[test]
    fn rejects_malformed_packets() {
        let good = build_query(7, "a.b", TYPE_A);
        for cut in 0..good.len() {
            assert!(parse_query(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut response = good.clone();
        response[2] |= 0x80;
        assert_eq!(parse_query(&response), Err(DnsError::NotQuery));
        let mut pointer = good.clone();
        pointer[12] = 0xc0;
        assert!(parse_query(&pointer).is_err());
        let mut two = good;
        two[5] = 2;
        assert!(parse_query(&two).is_err());
    }
}
