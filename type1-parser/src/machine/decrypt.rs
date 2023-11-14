use std::{
    iter::{Cloned, Enumerate},
    slice::Iter,
};

use winnow::stream::{Offset, Stream};

const C1: u16 = 52845;
const C2: u16 = 22719;

/// Decrypt byte by byte, using the algorithm described in the Type 1 Font
#[derive(Clone, Copy, Debug)]
struct Decryptor(u16);

impl Decryptor {
    fn decrypt(&mut self, b: u8) -> u8 {
        let r = b ^ (self.0 >> 8) as u8;
        self.0 = (b as u16 + self.0) * C1 + C2;
        r
    }
}

fn decrypt(key: u16, n: usize, buf: &[u8]) -> Vec<[u8]> {
    if buf.len() <= n {
        return vec![];
    }

    let mut decryptor = Decryptor(key);
    for b in &buf[..n] {
        decryptor.decrypt(*b);
    }
    buf[n..]
        .iter()
        .cloned()
        .map(|b| [decryptor.decrypt(b)])
        .collect()
}

struct IterOffsets<'a> {
    inner: Enumerate<Cloned<Iter<'a, u8>>>,
    decryptor: Decryptor,
}

impl<'a> Iterator for IterOffsets<'a> {
    type Item = (usize, u8);
    fn next(&mut self) -> Option<(usize, u8)> {
        self.inner
            .next()
            .map(|(offset, b)| (offset, self.decryptor.decrypt(b)))
    }
}

/// Implement winnow stream that decrypts data on the fly.
#[derive(Debug, Clone)]
pub struct DecryptStream<'a> {
    data: &'a [u8],
    decryptor: Decryptor,
}

impl<'a> Offset<DecryptStream<'a>> for DecryptStream<'a> {
    fn offset_from(&self, start: &Self) -> usize {
        self.data.offset_from(&start.data)
    }
}

impl<'a> DecryptStream<'a> {
    fn new(data: &'a [u8], key: u16, n: usize) -> Self {
        let mut decryptor = Decryptor(key);
        for b in &data[..n] {
            decryptor.decrypt(*b);
        }

        Self {
            data: &data[n..],
            decryptor,
        }
    }
}

impl<'a> Stream for DecryptStream<'a> {
    type Token = u8;
    type Slice = Box<[u8]>;
    type IterOffsets = IterOffsets<'a>;
    type Checkpoint = DecryptStream<'a>;

    fn iter_offsets(&self) -> Self::IterOffsets {
        IterOffsets {
            inner: self.data.iter_offsets(),
            decryptor: self.decryptor,
        }
    }

    fn eof_offset(&self) -> usize {
        self.data.eof_offset()
    }

    fn next_token(&mut self) -> Option<Self::Token> {
        self.data.next_token().map(|b| self.decryptor.decrypt(b))
    }

    fn offset_for<P>(&self, predicate: P) -> Option<usize>
    where
        P: Fn(Self::Token) -> bool,
    {
        let decryptor = self.decryptor;
        let predicate = move |b| predicate(decryptor.decrypt(b));
        self.data.offset_for(predicate)
    }

    fn offset_at(&self, tokens: usize) -> Result<usize, winnow::error::Needed> {
        self.data.offset_at(tokens)
    }

    fn next_slice(&mut self, offset: usize) -> Self::Slice {
        let mut r = self.data.next_slice(offset).to_owned();
        for b in &mut r {
            *b = self.decryptor.decrypt(*b);
        }
        r.into()
    }

    fn checkpoint(&self) -> Self::Checkpoint {
        self.clone()
    }

    fn reset(&mut self, checkpoint: Self::Checkpoint) {
        *self = checkpoint
    }

    fn raw(&self) -> &dyn std::fmt::Debug {
        &self.data
    }
}
