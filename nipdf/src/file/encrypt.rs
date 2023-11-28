use crate::object::ObjectId;
use arc4::Arc4;
use md5::{Digest, Md5};
use nipdf_macro::{pdf_object, TryFromIntObject};
use prescript::Name;
use tinyvec::{Array, ArrayVec, TinyVec};

#[derive(TryFromIntObject, Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum Algorithm {
    #[default]
    Undocument = 0,
    Aes = 1,
    AesV2 = 2,
    Unpublished = 3,
    DefinedInDoc = 4,
}

#[derive(TryFromIntObject, PartialEq, Eq, Debug, Clone, Copy, PartialOrd, Ord)]
pub enum StandardHandlerRevion {
    V2 = 2,
    V3 = 3,
    V4 = 4,
}

#[pdf_object(())]
pub trait EncryptDictTrait {
    fn filter(&self) -> Name;

    fn sub_filter(&self) -> Option<Name>;

    #[or_default]
    #[key("V")]
    #[try_from]
    fn algorithm(&self) -> Algorithm;

    #[key("Length")]
    #[default(40)]
    fn key_length(&self) -> u32;

    #[key("P")]
    fn permission_flags(&self) -> u32;

    #[key("R")]
    #[try_from]
    fn revison(&self) -> StandardHandlerRevion;

    /// 32-byte long string.
    #[key("O")]
    fn owner_password_hash(&self) -> &[u8];

    /// 32-byte long string.
    #[key("U")]
    fn user_password_hash(&self) -> &[u8];
}

const PADDING: [u8; 32] = [
    0x28u8, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01,
    0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69,
    0x7A,
];

/// Pad or truncate the password to 32 bytes.
/// If the password is longer than 32 bytes, the extra bytes are ignored.
/// If the password is shorter than 32 bytes, it is padded with bytes
/// [28 BF 4E 5E 4E 75 8A 41 64 00 4E 56 FF FA 01 08 2E 2E 00 B6 D0 68 3E 80 2F 0C A9 FE 64 53 69
/// 7A]
fn pad_trunc_password(s: &[u8]) -> [u8; 32] {
    let mut iter = s.iter().copied().chain(PADDING).take(32);
    std::array::from_fn(|_| iter.next().unwrap())
}

// algorithm 2
pub fn calc_encrypt_key(
    revion: StandardHandlerRevion,
    key_length: usize,
    user_password: &[u8],
    owner_hash: &[u8; 32],
    permission_flags: u32,
    doc_id: &[u8],
) -> Box<[u8]> {
    let user_password = pad_trunc_password(user_password);
    let mut md5 = Md5::new();
    md5.update(user_password);
    md5.update(owner_hash);
    md5.update(permission_flags.to_le_bytes());
    md5.update(doc_id);
    // md5.update(&[0xff, 0xff, 0xff, 0xff]);
    let mut hash = md5.finalize();
    let n = key_length / 8;
    if revion > StandardHandlerRevion::V2 {
        for _ in 0..50 {
            hash = Md5::digest(&hash[..n]);
        }
    }
    (&hash[..n]).into()
}

// algorithm 4 and 5
fn calc_user_hash(
    revion: StandardHandlerRevion,
    key_length: usize,
    user_password: &[u8],
    owner_hash: &[u8; 32],
    permission_flags: u32,
    doc_id: &[u8],
) -> [u8; 32] {
    let key = calc_encrypt_key(
        revion,
        key_length,
        user_password,
        owner_hash,
        permission_flags,
        doc_id,
    );

    if revion == StandardHandlerRevion::V2 {
        let mut r = PADDING.to_owned();
        Arc4::with_key(&key).encrypt(&mut r);
        r
    } else {
        let mut md5 = Md5::new();
        md5.update(PADDING);
        md5.update(doc_id);
        let mut hash = md5.finalize();
        let mut tmp = key.to_vec();
        for i in 0..=19 {
            for (t, k) in tmp.as_mut_slice().iter_mut().zip(&key[..]) {
                *t = *k ^ i;
            }
            Arc4::with_key(&tmp[..]).encrypt(&mut hash);
        }
        let mut r = [0u8; 32];
        r[..16].copy_from_slice(&hash[..]);
        r
    }
}

// algorithm 6
pub fn authorize_user(
    revion: StandardHandlerRevion,
    key_length: usize,
    user_password: &[u8],
    owner_hash: &[u8; 32],
    user_hash: &[u8; 32],
    permission_flags: u32,
    doc_id: &[u8],
) -> bool {
    let hash = calc_user_hash(
        revion,
        key_length,
        user_password,
        owner_hash,
        permission_flags,
        doc_id,
    );

    if revion == StandardHandlerRevion::V2 {
        &hash[..] == user_hash
    } else {
        hash[..16] == user_hash[..16]
    }
}

/// algorithm 3 step a to d
#[allow(dead_code)]
fn calc_rc4_key(
    revion: StandardHandlerRevion,
    key_length: usize,
    owner_password: &[u8],
) -> Box<[u8]> {
    let mut owner_password = Md5::digest(pad_trunc_password(owner_password));
    if revion > StandardHandlerRevion::V2 {
        for _ in 0..50 {
            owner_password = Md5::digest(&owner_password[..]);
        }
    }
    (&owner_password[..(key_length / 8)]).into()
}

// algorithm 7
#[allow(dead_code)]
fn authorize_owner(
    revion: StandardHandlerRevion,
    key_length: usize,
    owner_password: &[u8],
    owner_hash: &[u8; 32],
    user_hash: &[u8; 32],
    permission_flag: u32,
    doc_id: &[u8],
) -> bool {
    let rc4_key = &calc_rc4_key(revion, key_length, owner_password);
    let mut decrypt = owner_hash.to_vec();
    if revion == StandardHandlerRevion::V2 {
        Arc4::with_key(rc4_key).encrypt(&mut decrypt);
    } else {
        let mut tmp = rc4_key.to_vec();
        for i in (0..19).rev() {
            for (t, k) in tmp.as_mut_slice().iter_mut().zip(&rc4_key[..]) {
                *t = *k ^ i;
            }
            Arc4::with_key(&tmp[..]).encrypt(&mut decrypt);
        }
    }

    authorize_user(
        revion,
        key_length,
        &decrypt,
        owner_hash,
        user_hash,
        permission_flag,
        doc_id,
    )
}

pub trait VecLike {
    fn drain(&mut self, range: std::ops::Range<usize>) -> ArrayVec<[u8; 16]>;
    fn as_mut_slice(&mut self) -> &mut [u8];
}

pub trait Decryptor {
    fn new(key: &[u8], id: ObjectId) -> Self;
    fn decrypt<V: VecLike>(&self, data: &mut V);
}

impl<A> VecLike for TinyVec<A>
where
    A: Array<Item = u8>,
{
    fn drain(&mut self, range: std::ops::Range<usize>) -> ArrayVec<[u8; 16]> {
        self.drain(range).collect()
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl VecLike for Vec<u8> {
    fn drain(&mut self, range: std::ops::Range<usize>) -> ArrayVec<[u8; 16]> {
        self.drain(range).collect()
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

pub struct Rc4Decryptor(ArrayVec<[u8; 16]>);

impl Decryptor for Rc4Decryptor {
    fn new(key: &[u8], id: ObjectId) -> Self {
        let n = key.len();
        let mut k = TinyVec::<[u8; 16 + 5]>::with_capacity(n + 5);
        k.extend_from_slice(key);
        k.extend_from_slice(&u32::from(id.id()).to_le_bytes()[..3]);
        k.extend_from_slice(&id.generation().to_le_bytes()[..]);
        let key = Md5::digest(&k[..]);
        let key = key.into_iter().take((n + 5).min(16)).collect();
        Self(key)
    }

    fn decrypt<V: VecLike>(&self, data: &mut V) {
        Arc4::with_key(&self.0).encrypt(data.as_mut_slice());
    }
}

pub struct AesDecryptor(ArrayVec<[u8; 16]>);

impl Decryptor for AesDecryptor {
    fn new(key: &[u8], id: ObjectId) -> Self {
        let n = key.len();
        let mut k = TinyVec::<[u8; 16 + 5 + 4]>::with_capacity(n + 5 + 4);
        k.extend_from_slice(key);
        k.extend_from_slice(&u32::from(id.id()).to_le_bytes()[..3]);
        k.extend_from_slice(&id.generation().to_le_bytes()[..]);
        k.extend_from_slice(b"sAlT");
        let key = Md5::digest(&k[..]);
        let key = key.into_iter().take((n + 5).min(16)).collect();
        Self(key)
    }

    /// Decode data using aes, work in cbc mode,block size is 16 or Aes128.
    /// the initialization vector is a 16-byte random  number that is stored as the first 16 bytes
    /// of the encrypted data.
    /// Pad the data using the PKCS#5 padding scheme.
    fn decrypt<V: VecLike>(&self, data: &mut V) {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
        type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

        let iv_data = data.drain(0..16);
        let mut iv = [0u8; 16];
        iv.copy_from_slice(&iv_data[..]);
        Aes128CbcDec::new(self.0.as_ref().into(), &iv.into())
            .decrypt_padded_mut::<Pkcs7>(data.as_mut_slice())
            .unwrap();
    }
}

#[cfg(test)]
mod tests;
