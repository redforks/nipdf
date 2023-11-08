use nipdf_macro::{pdf_object, TryFromIntObject};

#[derive(TryFromIntObject, Default, PartialEq, Eq, Clone, Copy)]
enum Algorithm {
    #[default]
    Undocument = 0,
    AES = 1,
    AESV2 = 2,
    Unpublished = 3,
    DefinedInDoc = 4,
}

#[derive(TryFromIntObject, PartialEq, Eq, Clone, Copy)]
enum StandardHandlerRevion {
    V1 = 2,
    V2 = 3,
    V4 = 4,
}

#[pdf_object(())]
trait EncryptDictTrait {
    #[typ("Name")]
    fn filter(&self) -> &str;

    #[typ("Name")]
    fn sub_filter(&self) -> Option<&str>;

    #[or_default]
    #[key("V")]
    #[try_from]
    fn algorithm(&self) -> Algorithm;

    #[key("Length")]
    #[default(40)]
    fn key_length(&self) -> u32;

    #[key("R")]
    #[try_from]
    fn revison(&self) -> StandardHandlerRevion;

    /// 32-byte long string.
    #[key("O")]
    fn owner_password(&self) -> String;

    /// 32-byte long string.
    #[key("U")]
    fn user_password(&self) -> String;
}

const PADDING: [u8; 32] = [
    0x28u8, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01,
    0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69,
    0x7A,
];

/// Pad or truncate the password to 32 bytes.
/// If the password is longer than 32 bytes, the extra bytes are ignored.
/// If the password is shorter than 32 bytes, it is padded with bytes
/// [28 BF 4E 5E 4E 75 8A 41 64 00 4E 56 FF FA 01 08 2E 2E 00 B6 D0 68 3E 80 2F 0C A9 FE 64 53 69 7A]
fn pad_trunc_password(s: &[u8]) -> [u8; 32] {
    let mut iter = s.into_iter().copied().chain(PADDING.into_iter()).take(32);
    std::array::from_fn(|_| iter.next().unwrap())
}

#[cfg(test)]
mod tests;
