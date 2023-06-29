#[derive(Debug, thiserror::Error)]
pub enum Ascii85Error {
    #[error("tail error")]
    TailError,
}

pub fn decode(data: &[u8]) -> Result<Vec<u8>, Ascii85Error> {
    let mut out = Vec::with_capacity((data.len() + 4) / 5 * 4);

    let mut stream = data
        .iter()
        .cloned()
        .filter(|&b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'));

    let mut symbols = stream.by_ref().take_while(|&b| b != b'~');

    let (tail_len, tail) = loop {
        match symbols.next() {
            Some(b'z') => out.extend_from_slice(&[0; 4]),
            Some(a) => {
                let (b, c, d, e) = match (
                    symbols.next(),
                    symbols.next(),
                    symbols.next(),
                    symbols.next(),
                ) {
                    (Some(b), Some(c), Some(d), Some(e)) => (b, c, d, e),
                    (None, _, _, _) => break (1, [a, b'u', b'u', b'u', b'u']),
                    (Some(b), None, _, _) => break (2, [a, b, b'u', b'u', b'u']),
                    (Some(b), Some(c), None, _) => break (3, [a, b, c, b'u', b'u']),
                    (Some(b), Some(c), Some(d), None) => break (4, [a, b, c, d, b'u']),
                };
                out.extend_from_slice(&word_85([a, b, c, d, e]).ok_or(Ascii85Error::TailError)?);
            }
            None => break (0, [b'u'; 5]),
        }
    };

    if tail_len > 0 {
        let last = word_85(tail).ok_or(Ascii85Error::TailError)?;
        out.extend_from_slice(&last[..tail_len - 1]);
    }

    match (stream.next(), stream.next()) {
        (Some(b'>'), None) => Ok(out),
        _ => Err(Ascii85Error::TailError),
    }
}

fn sym_85(byte: u8) -> Option<u8> {
    match byte {
        b @ 0x21..=0x75 => Some(b - 0x21),
        _ => None,
    }
}

fn word_85([a, b, c, d, e]: [u8; 5]) -> Option<[u8; 4]> {
    fn s(b: u8) -> Option<u32> {
        sym_85(b).map(|n| n as u32)
    }
    let (a, b, c, d, e) = (s(a)?, s(b)?, s(c)?, s(d)?, s(e)?);
    let q = (((a * 85 + b) * 85 + c) * 85 + d) * 85 + e;
    Some(q.to_be_bytes())
}

#[cfg(test)]
mod tests;
