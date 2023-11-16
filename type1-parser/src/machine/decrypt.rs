use hex::decode;

const C1: u16 = 52845;
const C2: u16 = 22719;

pub const EEXEC_KEY: u16 = 55665;

/// Decrypt byte by byte, using the algorithm described in the Type 1 Font
#[derive(Clone, Copy, Debug)]
struct Decryptor(u16);

impl Decryptor {
    fn decrypt(&mut self, b: u8) -> u8 {
        let r = b ^ (self.0 >> 8) as u8;
        self.0 = ((b as u16).wrapping_add(self.0))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        r
    }
}

/// Returns (true, _) if data in hex form, (false, _) if in binary form.
pub fn decrypt(key: u16, n: usize, buf: &[u8]) -> (bool, Vec<u8>) {
    // check first 8 bytes of buf to see its format, assert that it is not ascii hex form
    let decoded_hex;
    let is_hex;
    let buf = if buf[..8].iter().all(|b| b.is_ascii_hexdigit()) {
        is_hex = true;
        // take slice until non ascii_hexdigit, if in odd number, trunk last digit
        let n = buf.iter().take_while(|b| b.is_ascii_hexdigit()).count();
        decoded_hex = decode(&buf[..n / 2 * 2]).unwrap();
        &decoded_hex[..]
    } else {
        is_hex = false;
        buf
    };

    let mut decryptor = Decryptor(key);
    for b in &buf[..n] {
        decryptor.decrypt(*b);
    }

    (
        is_hex,
        buf[n..]
            .iter()
            .cloned()
            .map(|b| decryptor.decrypt(b))
            .collect(),
    )
}
