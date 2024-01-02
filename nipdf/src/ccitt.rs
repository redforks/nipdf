use bitstream_io::{
    huffman::{compile_read_tree, ReadHuffmanTree},
    read::{BitRead, BitReader},
    BigEndian, HuffmanRead,
};
use bitvec::{prelude::Msb0, slice::BitSlice, vec::BitVec};
use educe::Educe;
use log::error;
use std::iter::repeat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Group3_1D,

    /// Mixed one- and two-dimensional encoding (Group 3,
    /// 2-D), in which a line encoded one-dimensionally may
    /// be followed by at most K − 1 lines encoded two-
    /// dimensionally
    Group3_2D(u16),

    Group4,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Code {
    Pass,
    Horizontal(Run, Run), // a0a1, a1a2
    Vertical(i8),
    Extension(u8),
    EndOfFassimileBlock,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Run {
    color: u8,
    bytes: u16,
}

impl Run {
    fn from(color: Color, bytes: u16) -> Self {
        Self::new(
            match color {
                Color::White => WHITE,
                Color::Black => BLACK,
            },
            bytes,
        )
    }

    fn new(color: u8, bytes: u16) -> Self {
        Self { color, bytes }
    }

    fn b_color(&self) -> bool {
        self.color == WHITE
    }
}

const BLACK: u8 = 0;
const B_BLACK: bool = false;
const WHITE: u8 = 255;
const B_WHITE: bool = true;
const GRAY: u8 = 128;
const NOT_USED: u8 = 100;
const EOL: u8 = 101;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("IOError: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Horizontal run color mismatch")]
    HorizontalRunColorMismatch,
    #[error("Unknown code")]
    InvalidCode,
}

type Result<T> = std::result::Result<T, DecodeError>;

struct RunHuffamnTree {
    black: Box<[ReadHuffmanTree<BigEndian, Run>]>,
    white: Box<[ReadHuffmanTree<BigEndian, Run>]>,
}

impl RunHuffamnTree {
    fn get(&self, color: Color) -> &[ReadHuffmanTree<BigEndian, Run>] {
        match color {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
}

#[rustfmt::skip]
fn build_run_huffman(algo: Algorithm) -> RunHuffamnTree {
    let mut white_codes = vec![
            (Run::new(WHITE, 0), vec![0, 0, 1, 1, 0, 1, 0, 1]),
            (Run::new(WHITE, 1), vec![0, 0, 0, 1, 1, 1]),
            (Run::new(WHITE, 2), vec![0, 1, 1, 1]),
            (Run::new(WHITE, 3), vec![1, 0, 0, 0]),
            (Run::new(WHITE, 4), vec![1, 0, 1, 1]),
            (Run::new(WHITE, 5), vec![1, 1, 0, 0]),
            (Run::new(WHITE, 6), vec![1, 1, 1, 0]),
            (Run::new(WHITE, 7), vec![1, 1, 1, 1]),
            (Run::new(WHITE, 8), vec![1, 0, 0, 1, 1]),
            (Run::new(WHITE, 9), vec![1, 0, 1, 0, 0]),
            (Run::new(WHITE, 10), vec![0, 0, 1, 1, 1]),
            (Run::new(WHITE, 11), vec![0, 1, 0, 0, 0]),
            (Run::new(WHITE, 12), vec![0, 0, 1, 0, 0, 0]),
            (Run::new(WHITE, 13), vec![0, 0, 0, 0, 1, 1]),
            (Run::new(WHITE, 14), vec![1, 1, 0, 1, 0, 0]),
            (Run::new(WHITE, 15), vec![1, 1, 0, 1, 0, 1]),
            (Run::new(WHITE, 16), vec![1, 0, 1, 0, 1, 0]),
            (Run::new(WHITE, 17), vec![1, 0, 1, 0, 1, 1]),
            (Run::new(WHITE, 18), vec![0, 1, 0, 0, 1, 1, 1]),
            (Run::new(WHITE, 19), vec![0, 0, 0, 1, 1, 0, 0]),
            (Run::new(WHITE, 20), vec![0, 0, 0, 1, 0, 0, 0]),
            (Run::new(WHITE, 21), vec![0, 0, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 22), vec![0, 0, 0, 0, 0, 1, 1]),
            (Run::new(WHITE, 23), vec![0, 0, 0, 0, 1, 0, 0]),
            (Run::new(WHITE, 24), vec![0, 1, 0, 1, 0, 0, 0]),
            (Run::new(WHITE, 25), vec![0, 1, 0, 1, 0, 1, 1]),
            (Run::new(WHITE, 26), vec![0, 0, 1, 0, 0, 1, 1]),
            (Run::new(WHITE, 27), vec![0, 1, 0, 0, 1, 0, 0]),
            (Run::new(WHITE, 28), vec![0, 0, 1, 1, 0, 0, 0]),
            (Run::new(WHITE, 29), vec![0, 0, 0, 0, 0, 0, 1, 0]),
            (Run::new(WHITE, 30), vec![0, 0, 0, 0, 0, 0, 1, 1]),
            (Run::new(WHITE, 31), vec![0, 0, 0, 1, 1, 0, 1, 0]),
            (Run::new(WHITE, 32), vec![0, 0, 0, 1, 1, 0, 1, 1]),
            (Run::new(WHITE, 33), vec![0, 0, 0, 1, 0, 0, 1, 0]),
            (Run::new(WHITE, 34), vec![0, 0, 0, 1, 0, 0, 1, 1]),
            (Run::new(WHITE, 35), vec![0, 0, 0, 1, 0, 1, 0, 0]),
            (Run::new(WHITE, 36), vec![0, 0, 0, 1, 0, 1, 0, 1]),
            (Run::new(WHITE, 37), vec![0, 0, 0, 1, 0, 1, 1, 0]),
            (Run::new(WHITE, 38), vec![0, 0, 0, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 39), vec![0, 0, 1, 0, 1, 0, 0, 0]),
            (Run::new(WHITE, 40), vec![0, 0, 1, 0, 1, 0, 0, 1]),
            (Run::new(WHITE, 41), vec![0, 0, 1, 0, 1, 0, 1, 0]),
            (Run::new(WHITE, 42), vec![0, 0, 1, 0, 1, 0, 1, 1]),
            (Run::new(WHITE, 43), vec![0, 0, 1, 0, 1, 1, 0, 0]),
            (Run::new(WHITE, 44), vec![0, 0, 1, 0, 1, 1, 0, 1]),
            (Run::new(WHITE, 45), vec![0, 0, 0, 0, 0, 1, 0, 0]),
            (Run::new(WHITE, 46), vec![0, 0, 0, 0, 0, 1, 0, 1]),
            (Run::new(WHITE, 47), vec![0, 0, 0, 0, 1, 0, 1, 0]),
            (Run::new(WHITE, 48), vec![0, 0, 0, 0, 1, 0, 1, 1]),
            (Run::new(WHITE, 49), vec![0, 1, 0, 1, 0, 0, 1, 0]),
            (Run::new(WHITE, 50), vec![0, 1, 0, 1, 0, 0, 1, 1]),
            (Run::new(WHITE, 51), vec![0, 1, 0, 1, 0, 1, 0, 0]),
            (Run::new(WHITE, 52), vec![0, 1, 0, 1, 0, 1, 0, 1]),
            (Run::new(WHITE, 53), vec![0, 0, 1, 0, 0, 1, 0, 0]),
            (Run::new(WHITE, 54), vec![0, 0, 1, 0, 0, 1, 0, 1]),
            (Run::new(WHITE, 55), vec![0, 1, 0, 1, 1, 0, 0, 0]),
            (Run::new(WHITE, 56), vec![0, 1, 0, 1, 1, 0, 0, 1]),
            (Run::new(WHITE, 57), vec![0, 1, 0, 1, 1, 0, 1, 0]),
            (Run::new(WHITE, 58), vec![0, 1, 0, 1, 1, 0, 1, 1]),
            (Run::new(WHITE, 59), vec![0, 1, 0, 0, 1, 0, 1, 0]),
            (Run::new(WHITE, 60), vec![0, 1, 0, 0, 1, 0, 1, 1]),
            (Run::new(WHITE, 61), vec![0, 0, 1, 1, 0, 0, 1, 0]),
            (Run::new(WHITE, 62), vec![0, 0, 1, 1, 0, 0, 1, 1]),
            (Run::new(WHITE, 63), vec![0, 0, 1, 1, 0, 1, 0, 0]),
            (Run::new(WHITE, 64), vec![1, 1, 0, 1, 1]),
            (Run::new(WHITE, 128), vec![1, 0, 0, 1, 0]),
            (Run::new(WHITE, 192), vec![0, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 256), vec![0, 1, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 320), vec![0, 0, 1, 1, 0, 1, 1, 0]),
            (Run::new(WHITE, 384), vec![0, 0, 1, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 448), vec![0, 1, 1, 0, 0, 1, 0, 0]),
            (Run::new(WHITE, 512), vec![0, 1, 1, 0, 0, 1, 0, 1]),
            (Run::new(WHITE, 576), vec![0, 1, 1, 0, 1, 0, 0, 0]),
            (Run::new(WHITE, 640), vec![0, 1, 1, 0, 0, 1, 1, 1]),
            (Run::new(WHITE, 704), vec![0, 1, 1, 0, 0, 1, 1, 0, 0]),
            (Run::new(WHITE, 768), vec![0, 1, 1, 0, 0, 1, 1, 0, 1]),
            (Run::new(WHITE, 832), vec![0, 1, 1, 0, 1, 0, 0, 1, 0]),
            (Run::new(WHITE, 896), vec![0, 1, 1, 0, 1, 0, 0, 1, 1]),
            (Run::new(WHITE, 960), vec![0, 1, 1, 0, 1, 0, 1, 0, 0]),
            (Run::new(WHITE, 1024), vec![0, 1, 1, 0, 1, 0, 1, 0, 1]),
            (Run::new(WHITE, 1088), vec![0, 1, 1, 0, 1, 0, 1, 1, 0]),
            (Run::new(WHITE, 1152), vec![0, 1, 1, 0, 1, 0, 1, 1, 1]),
            (Run::new(WHITE, 1216), vec![0, 1, 1, 0, 1, 1, 0, 0, 0]),
            (Run::new(WHITE, 1280), vec![0, 1, 1, 0, 1, 1, 0, 0, 1]),
            (Run::new(WHITE, 1344), vec![0, 1, 1, 0, 1, 1, 0, 1, 0]),
            (Run::new(WHITE, 1408), vec![0, 1, 1, 0, 1, 1, 0, 1, 1]),
            (Run::new(WHITE, 1472), vec![0, 1, 0, 0, 1, 1, 0, 0, 0]),
            (Run::new(WHITE, 1536), vec![0, 1, 0, 0, 1, 1, 0, 0, 1]),
            (Run::new(WHITE, 1600), vec![0, 1, 0, 0, 1, 1, 0, 1, 0]),
            (Run::new(WHITE, 1664), vec![0, 1, 1, 0, 0, 0]),
            (Run::new(WHITE, 1728), vec![0, 1, 0, 0, 1, 1, 0, 1, 1]),
            (Run::new(GRAY, 1792), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (Run::new(GRAY, 1856), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0]),
            (Run::new(GRAY, 1920), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1]),
            (Run::new(GRAY, 1984), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0]),
            (Run::new(GRAY, 2048), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1]),
            (Run::new(GRAY, 2112), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
            (Run::new(GRAY, 2176), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1]),
            (Run::new(GRAY, 2240), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0]),
            (Run::new(GRAY, 2304), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (Run::new(GRAY, 2368), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0]),
            (Run::new(GRAY, 2432), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1]),
            (Run::new(GRAY, 2496), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0]),
            (Run::new(GRAY, 2560), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1]),
            (Run::new(NOT_USED, 0), vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];

    let mut black_codes = vec![
            (Run::new(BLACK, 0), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 1), vec![0, 1, 0]),
            (Run::new(BLACK, 2), vec![1, 1]),
            (Run::new(BLACK, 3), vec![1, 0]),
            (Run::new(BLACK, 4), vec![0, 1, 1]),
            (Run::new(BLACK, 5), vec![0, 0, 1, 1]),
            (Run::new(BLACK, 6), vec![0, 0, 1, 0]),
            (Run::new(BLACK, 7), vec![0, 0, 0, 1, 1]),
            (Run::new(BLACK, 8), vec![0, 0, 0, 1, 0, 1]),
            (Run::new(BLACK, 9), vec![0, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 10), vec![0, 0, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 11), vec![0, 0, 0, 0, 1, 0, 1]),
            (Run::new(BLACK, 12), vec![0, 0, 0, 0, 1, 1, 1]),
            (Run::new(BLACK, 13), vec![0, 0, 0, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 14), vec![0, 0, 0, 0, 0, 1, 1, 1]),
            (Run::new(BLACK, 15), vec![0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (Run::new(BLACK, 16), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 17), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (Run::new(BLACK, 18), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 19), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1]),
            (Run::new(BLACK, 20), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 21), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 22), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 23), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 24), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 25), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (Run::new(BLACK, 26), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0]),
            (Run::new(BLACK, 27), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1]),
            (Run::new(BLACK, 28), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 29), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1]),
            (Run::new(BLACK, 30), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 31), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1]),
            (Run::new(BLACK, 32), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
            (Run::new(BLACK, 33), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1]),
            (Run::new(BLACK, 34), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0]),
            (Run::new(BLACK, 35), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1]),
            (Run::new(BLACK, 36), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0]),
            (Run::new(BLACK, 37), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1]),
            (Run::new(BLACK, 38), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0]),
            (Run::new(BLACK, 39), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 40), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 41), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1]),
            (Run::new(BLACK, 42), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 0]),
            (Run::new(BLACK, 43), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1]),
            (Run::new(BLACK, 44), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0]),
            (Run::new(BLACK, 45), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
            (Run::new(BLACK, 46), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0]),
            (Run::new(BLACK, 47), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 48), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 49), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1]),
            (Run::new(BLACK, 50), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0]),
            (Run::new(BLACK, 51), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1]),
            (Run::new(BLACK, 52), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 53), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 54), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0]),
            (Run::new(BLACK, 55), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1]),
            (Run::new(BLACK, 56), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 57), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0]),
            (Run::new(BLACK, 58), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1]),
            (Run::new(BLACK, 59), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1]),
            (Run::new(BLACK, 60), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 61), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0]),
            (Run::new(BLACK, 62), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
            (Run::new(BLACK, 63), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1]),
            (Run::new(BLACK, 64), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 1]),
            (Run::new(BLACK, 128), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0]),
            (Run::new(BLACK, 192), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1]),
            (Run::new(BLACK, 256), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1]),
            (Run::new(BLACK, 320), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1]),
            (Run::new(BLACK, 384), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0]),
            (Run::new(BLACK, 448), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1]),
            (Run::new(BLACK, 512), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 576), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1]),
            (Run::new(BLACK, 640), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0]),
            (Run::new(BLACK, 704), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 1]),
            (Run::new(BLACK, 768), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0]),
            (Run::new(BLACK, 832), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1]),
            (Run::new(BLACK, 896), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0]),
            (Run::new(BLACK, 960), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1]),
            (Run::new(BLACK, 1024), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0]),
            (Run::new(BLACK, 1088), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1]),
            (Run::new(BLACK, 1152), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0]),
            (Run::new(BLACK, 1216), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1]),
            (Run::new(BLACK, 1280), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0]),
            (Run::new(BLACK, 1344), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1]),
            (Run::new(BLACK, 1408), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0]),
            (Run::new(BLACK, 1472), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
            (Run::new(BLACK, 1536), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0]),
            (Run::new(BLACK, 1600), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1]),
            (Run::new(BLACK, 1664), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0]),
            (Run::new(BLACK, 1728), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1]),
            (Run::new(GRAY, 1792), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (Run::new(GRAY, 1856), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0]),
            (Run::new(GRAY, 1920), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1]),
            (Run::new(GRAY, 1984), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0]),
            (Run::new(GRAY, 2048), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1]),
            (Run::new(GRAY, 2112), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
            (Run::new(GRAY, 2176), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1]),
            (Run::new(GRAY, 2240), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0]),
            (Run::new(GRAY, 2304), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (Run::new(GRAY, 2368), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0]),
            (Run::new(GRAY, 2432), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1]),
            (Run::new(GRAY, 2496), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0]),
            (Run::new(GRAY, 2560), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1]),
            (Run::new(NOT_USED, 0), vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];
    
    match algo {
        Algorithm::Group3_1D => {
            let len = white_codes.len();
            white_codes[len - 1]=(Run::new(EOL, 0), vec![0,0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            let len = black_codes.len();
            black_codes[len - 1] = (Run::new(EOL, 0), vec![0,0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        }
        Algorithm::Group3_2D(_) => todo!(),
        Algorithm::Group4 => {},
    }

    RunHuffamnTree {
        white: compile_read_tree(white_codes).unwrap(),
        black: compile_read_tree(black_codes).unwrap(),
    }
}

fn next_run(
    reader: &mut impl HuffmanRead<BigEndian>,
    huffman: &RunHuffamnTree,
    color: Color,
) -> Result<Run> {
    let tree = huffman.get(color);
    let mut r = Run::from(color, 0);
    loop {
        let run = reader.read_huffman(tree)?;
        match run.color {
            GRAY => {}
            BLACK | WHITE => {
                let b_color = match color {
                    Color::Black => BLACK,
                    Color::White => WHITE,
                };
                if !(run.color == BLACK || run.color == WHITE || run.color == b_color) {
                    return Err(DecodeError::HorizontalRunColorMismatch);
                }
            }
            _ => unreachable!(),
        }
        r.bytes += run.bytes;
        if run.bytes < 64 {
            return Ok(r);
        }
    }
}

fn iter_code(
    algo: Algorithm,
    buf: &[u8],
) -> impl FnMut(State, &Flags) -> Option<Result<Code>> + '_ {
    let huffman = build_run_huffman(algo);
    fn next(
        huffman: &RunHuffamnTree,
        reader: &mut (impl BitRead + HuffmanRead<BigEndian>),
        state: State,
        flags: &Flags,
    ) -> Result<Code> {
        if flags.encoded_byte_align && state.is_new_line {
            reader.byte_align();
        }

        if reader.read_bit()? {
            // 1
            return Ok(Code::Vertical(0));
        }

        match reader.read::<u8>(2)? {
            0b11 => Ok(Code::Vertical(1)),  // 011
            0b10 => Ok(Code::Vertical(-1)), // 010
            0b01 => {
                let a0a1 = next_run(reader, huffman, state.color)?;
                let a1a2 = next_run(reader, huffman, state.color.toggle())?;
                Ok(Code::Horizontal(a0a1, a1a2))
            }
            0b00 => {
                if reader.read_bit()? {
                    // 0001
                    Ok(Code::Pass)
                } else {
                    // 0000
                    match reader.read::<u8>(2)? {
                        0b11 => Ok(Code::Vertical(2)), // 000011
                        0b01 => match reader.read_bit()? {
                            // 0000_01
                            true => Ok(Code::Vertical(3)),   // 0000011
                            false => Ok(Code::Vertical(-3)), // 0000010
                        },
                        0b00 => match reader.read_bit()? {
                            true => {
                                // 0000_001
                                let ext = reader.read::<u8>(3)?;
                                Ok(Code::Extension(ext))
                            }
                            false => {
                                // 0000_000
                                if reader.read::<u8>(5)? == 1
                                    && reader.read::<u8>(4)? == 0
                                    && reader.read::<u8>(8)? == 1
                                {
                                    Ok(Code::EndOfFassimileBlock)
                                } else {
                                    Err(DecodeError::InvalidCode)
                                }
                            }
                        },
                        0b10 => Ok(Code::Vertical(-2)), // 000010
                        _ => unreachable!(),
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    let mut reader = BitReader::endian(buf, BigEndian);
    move |state, flags| match next(&huffman, &mut reader, state, flags) {
        Ok(v) => Some(Ok(v)),
        Err(e) => match e {
            DecodeError::IOError(io_err) => {
                if io_err.kind() == std::io::ErrorKind::UnexpectedEof {
                    None
                } else {
                    Some(Err(io_err.into()))
                }
            }
            _ => Some(Err(e)),
        },
    }
}

struct LineBuf<'a>(&'a BitSlice<u8, Msb0>);

impl<'a> LineBuf<'a> {
    fn b1(&self, pos: Option<usize>, pos_color: bool) -> usize {
        let pos = self.next_flip(pos);
        if pos < self.0.len() && self.0[pos] == pos_color {
            self.next_flip(Some(pos))
        } else {
            pos
        }
    }

    fn next_flip(&self, pos: Option<usize>) -> usize {
        let color = match pos {
            None => true,
            Some(pos) => self.0[pos],
        };
        let pos = pos.unwrap_or_default();
        if pos == self.0.len() {
            return pos;
        }

        self.0[pos..]
            .iter()
            .position(|c| c != color)
            .map_or(self.0.len(), |p| pos + p)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
enum Color {
    Black,
    #[default]
    White,
}

impl Color {
    pub fn toggle(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Default)]
struct State {
    color: Color,
    is_new_line: bool,
}

impl State {
    pub fn toggle_color(self) -> Self {
        Self {
            color: self.color.toggle(),
            is_new_line: self.is_new_line,
        }
    }

    pub fn toggle_is_new_line(self) -> Self {
        Self {
            color: self.color,
            is_new_line: !self.is_new_line,
        }
    }
}

struct CoderGroup4<'a> {
    last: LineBuf<'a>,
    cur: &'a mut BitSlice<u8, Msb0>,
    cur_color: Color,
    pos: Option<usize>,
}

impl<'a> CoderGroup4<'a> {
    fn new(last: &'a BitSlice<u8, Msb0>, cur: &'a mut BitSlice<u8, Msb0>) -> Self {
        debug_assert!(last.len() == cur.len());
        Self {
            last: LineBuf(last),
            cur,
            cur_color: Color::White,
            pos: None,
        }
    }

    fn is_new_line(&self) -> bool {
        self.pos.is_none()
    }

    fn cur_color(&self) -> bool {
        self.cur_color == Color::White
    }

    fn fill(&mut self, run: Run) {
        let mut pos = self.pos.unwrap_or_default();
        for _ in 0..run.bytes {
            self.cur.set(pos, run.b_color());
            pos += 1;
        }
        self.pos = Some(pos);
    }

    pub fn state(&self) -> State {
        // TODO: state to be a field of CoderGroup4
        State {
            color: self.cur_color,
            is_new_line: self.is_new_line(),
        }
    }

    // return true if current line filled.
    #[allow(clippy::cast_possible_truncation)]
    fn decode(&mut self, code: Code) -> Result<bool> {
        match code {
            Code::Horizontal(a0a1, a1a2) => {
                self.fill(a0a1);
                self.fill(a1a2);
            }
            Code::Vertical(n) => {
                let b1 = self.last.b1(self.pos, self.cur_color == Color::White);
                self.fill(Run::from(
                    self.cur_color,
                    (b1 as i16 - self.pos.unwrap_or_default() as i16 + n as i16) as u16,
                ));
                self.cur_color = self.cur_color.toggle();
            }
            Code::Pass => {
                let b1 = self.last.b1(self.pos, self.cur_color == Color::White);
                let b2 = self.last.next_flip(Some(b1));
                self.fill(Run::from(
                    self.cur_color,
                    (b2 - self.pos.unwrap_or_default()) as u16,
                ));
                debug_assert_eq!(self.pos.unwrap(), b2);
            }
            _ => unreachable!(),
        };
        debug_assert!(self.pos.unwrap_or_default() <= self.cur.len());
        Ok(self.pos.unwrap_or_default() == self.cur.len())
    }
}

#[derive(Debug, Educe)]
#[educe(Default)]
pub struct Flags {
    pub encoded_byte_align: bool,
    pub inverse_black_white: bool,
    #[educe(Default = true)]
    pub end_of_block: bool,
}

pub struct Decoder {
    pub algorithm: Algorithm,
    pub width: u16,
    pub rows: Option<u16>,
    pub flags: Flags,
}

impl Decoder {
    pub fn decode(&self, buf: &[u8]) -> Result<Vec<u8>> {
        assert!(!matches!(self.algorithm, Algorithm::Group3_2D(_)));
        let image_line: BitVec<u8, Msb0> = repeat(B_WHITE).take(self.width as usize).collect();
        let last_line = &image_line[..];
        let mut r = BitVec::<u8, Msb0>::with_capacity(
            self.rows.unwrap_or(30) as usize * self.width as usize,
        );
        let mut line_buf: BitVec<u8, Msb0> = repeat(true).take(self.width as usize).collect();
        let mut next_code = iter_code(self.algorithm, buf);
        let mut coder = CoderGroup4::new(last_line, &mut line_buf);
        loop {
            let code = next_code(coder.state(), &self.flags);
            match code {
                None => break,
                Some(code) => match code? {
                    Code::Extension(_) => todo!(),
                    Code::EndOfFassimileBlock
                        if self.flags.end_of_block && self.algorithm == Algorithm::Group4 =>
                    {
                        break;
                    }
                    code => {
                        if coder.decode(code)? {
                            r.extend(line_buf.iter());
                            if !self.flags.end_of_block {
                                if let Some(rows) = self.rows {
                                    if rows as usize == r.len() / self.width as usize {
                                        break;
                                    }
                                }
                            }
                            coder = CoderGroup4::new(
                                &r[r.len() - self.width as usize..],
                                &mut line_buf,
                            );
                        }
                    }
                },
            }
        }

        if self.flags.inverse_black_white {
            for byte in r.as_raw_mut_slice() {
                *byte = !*byte;
            }
        }
        Ok(r.into_vec())
    }
}

#[allow(dead_code)]
fn write_buf(buf: &[u8], width: usize) {
    // write buf content to /tmp/foo, white as '1', black as '0'
    use std::{fs::File, io::Write};

    let mut f = File::create("/tmp/foo").unwrap();
    for line in buf.chunks(width) {
        for &c in line {
            let c = if c == WHITE { '1' } else { '0' };
            write!(f, "{}", c).unwrap();
        }
        writeln!(f).unwrap();
    }
}

#[cfg(test)]
mod tests;
