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
    Horizontal(PictualElement, PictualElement), // a0a1, a1a2
    Vertical(i8),
    Extension(u8),
    EndOfFassimileBlock,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PictualElement {
    Black(u16),
    White(u16),
    MakeUp(u16),
    EOL,

    /// Unused code in huffman tree
    Unused,
}

impl PictualElement {
    pub fn from_color(color: Color, bytes: u16) -> Self {
        match color {
            Color::Black => Self::Black(bytes),
            Color::White => Self::White(bytes),
        }
    }

    pub fn is_white(&self) -> bool {
        match self {
            Self::White(_) => true,
            Self::Black(_) => false,
            _ => unreachable!(),
        }
    }

    pub fn len(self) -> u16 {
        match self {
            Self::Black(len) | Self::White(len) | Self::MakeUp(len) => len,
            _ => unreachable!(),
        }
    }
}

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
    black: Box<[ReadHuffmanTree<BigEndian, PictualElement>]>,
    white: Box<[ReadHuffmanTree<BigEndian, PictualElement>]>,
}

impl RunHuffamnTree {
    fn get(&self, color: Color) -> &[ReadHuffmanTree<BigEndian, PictualElement>] {
        match color {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
}

#[rustfmt::skip]
fn build_run_huffman(algo: Algorithm) -> RunHuffamnTree {
    let mut white_codes = vec![
            (PictualElement::White(0), vec![0, 0, 1, 1, 0, 1, 0, 1]),
            (PictualElement::White(1), vec![0, 0, 0, 1, 1, 1]),
            (PictualElement::White(2), vec![0, 1, 1, 1]),
            (PictualElement::White(3), vec![1, 0, 0, 0]),
            (PictualElement::White(4), vec![1, 0, 1, 1]),
            (PictualElement::White(5), vec![1, 1, 0, 0]),
            (PictualElement::White(6), vec![1, 1, 1, 0]),
            (PictualElement::White(7), vec![1, 1, 1, 1]),
            (PictualElement::White(8), vec![1, 0, 0, 1, 1]),
            (PictualElement::White(9), vec![1, 0, 1, 0, 0]),
            (PictualElement::White(10), vec![0, 0, 1, 1, 1]),
            (PictualElement::White(11), vec![0, 1, 0, 0, 0]),
            (PictualElement::White(12), vec![0, 0, 1, 0, 0, 0]),
            (PictualElement::White(13), vec![0, 0, 0, 0, 1, 1]),
            (PictualElement::White(14), vec![1, 1, 0, 1, 0, 0]),
            (PictualElement::White(15), vec![1, 1, 0, 1, 0, 1]),
            (PictualElement::White(16), vec![1, 0, 1, 0, 1, 0]),
            (PictualElement::White(17), vec![1, 0, 1, 0, 1, 1]),
            (PictualElement::White(18), vec![0, 1, 0, 0, 1, 1, 1]),
            (PictualElement::White(19), vec![0, 0, 0, 1, 1, 0, 0]),
            (PictualElement::White(20), vec![0, 0, 0, 1, 0, 0, 0]),
            (PictualElement::White(21), vec![0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::White(22), vec![0, 0, 0, 0, 0, 1, 1]),
            (PictualElement::White(23), vec![0, 0, 0, 0, 1, 0, 0]),
            (PictualElement::White(24), vec![0, 1, 0, 1, 0, 0, 0]),
            (PictualElement::White(25), vec![0, 1, 0, 1, 0, 1, 1]),
            (PictualElement::White(26), vec![0, 0, 1, 0, 0, 1, 1]),
            (PictualElement::White(27), vec![0, 1, 0, 0, 1, 0, 0]),
            (PictualElement::White(28), vec![0, 0, 1, 1, 0, 0, 0]),
            (PictualElement::White(29), vec![0, 0, 0, 0, 0, 0, 1, 0]),
            (PictualElement::White(30), vec![0, 0, 0, 0, 0, 0, 1, 1]),
            (PictualElement::White(31), vec![0, 0, 0, 1, 1, 0, 1, 0]),
            (PictualElement::White(32), vec![0, 0, 0, 1, 1, 0, 1, 1]),
            (PictualElement::White(33), vec![0, 0, 0, 1, 0, 0, 1, 0]),
            (PictualElement::White(34), vec![0, 0, 0, 1, 0, 0, 1, 1]),
            (PictualElement::White(35), vec![0, 0, 0, 1, 0, 1, 0, 0]),
            (PictualElement::White(36), vec![0, 0, 0, 1, 0, 1, 0, 1]),
            (PictualElement::White(37), vec![0, 0, 0, 1, 0, 1, 1, 0]),
            (PictualElement::White(38), vec![0, 0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::White(39), vec![0, 0, 1, 0, 1, 0, 0, 0]),
            (PictualElement::White(40), vec![0, 0, 1, 0, 1, 0, 0, 1]),
            (PictualElement::White(41), vec![0, 0, 1, 0, 1, 0, 1, 0]),
            (PictualElement::White(42), vec![0, 0, 1, 0, 1, 0, 1, 1]),
            (PictualElement::White(43), vec![0, 0, 1, 0, 1, 1, 0, 0]),
            (PictualElement::White(44), vec![0, 0, 1, 0, 1, 1, 0, 1]),
            (PictualElement::White(45), vec![0, 0, 0, 0, 0, 1, 0, 0]),
            (PictualElement::White(46), vec![0, 0, 0, 0, 0, 1, 0, 1]),
            (PictualElement::White(47), vec![0, 0, 0, 0, 1, 0, 1, 0]),
            (PictualElement::White(48), vec![0, 0, 0, 0, 1, 0, 1, 1]),
            (PictualElement::White(49), vec![0, 1, 0, 1, 0, 0, 1, 0]),
            (PictualElement::White(50), vec![0, 1, 0, 1, 0, 0, 1, 1]),
            (PictualElement::White(51), vec![0, 1, 0, 1, 0, 1, 0, 0]),
            (PictualElement::White(52), vec![0, 1, 0, 1, 0, 1, 0, 1]),
            (PictualElement::White(53), vec![0, 0, 1, 0, 0, 1, 0, 0]),
            (PictualElement::White(54), vec![0, 0, 1, 0, 0, 1, 0, 1]),
            (PictualElement::White(55), vec![0, 1, 0, 1, 1, 0, 0, 0]),
            (PictualElement::White(56), vec![0, 1, 0, 1, 1, 0, 0, 1]),
            (PictualElement::White(57), vec![0, 1, 0, 1, 1, 0, 1, 0]),
            (PictualElement::White(58), vec![0, 1, 0, 1, 1, 0, 1, 1]),
            (PictualElement::White(59), vec![0, 1, 0, 0, 1, 0, 1, 0]),
            (PictualElement::White(60), vec![0, 1, 0, 0, 1, 0, 1, 1]),
            (PictualElement::White(61), vec![0, 0, 1, 1, 0, 0, 1, 0]),
            (PictualElement::White(62), vec![0, 0, 1, 1, 0, 0, 1, 1]),
            (PictualElement::White(63), vec![0, 0, 1, 1, 0, 1, 0, 0]),
            (PictualElement::White(64), vec![1, 1, 0, 1, 1]),
            (PictualElement::White(128), vec![1, 0, 0, 1, 0]),
            (PictualElement::White(192), vec![0, 1, 0, 1, 1, 1]),
            (PictualElement::White(256), vec![0, 1, 1, 0, 1, 1, 1]),
            (PictualElement::White(320), vec![0, 0, 1, 1, 0, 1, 1, 0]),
            (PictualElement::White(384), vec![0, 0, 1, 1, 0, 1, 1, 1]),
            (PictualElement::White(448), vec![0, 1, 1, 0, 0, 1, 0, 0]),
            (PictualElement::White(512), vec![0, 1, 1, 0, 0, 1, 0, 1]),
            (PictualElement::White(576), vec![0, 1, 1, 0, 1, 0, 0, 0]),
            (PictualElement::White(640), vec![0, 1, 1, 0, 0, 1, 1, 1]),
            (PictualElement::White(704), vec![0, 1, 1, 0, 0, 1, 1, 0, 0]),
            (PictualElement::White(768), vec![0, 1, 1, 0, 0, 1, 1, 0, 1]),
            (PictualElement::White(832), vec![0, 1, 1, 0, 1, 0, 0, 1, 0]),
            (PictualElement::White(896), vec![0, 1, 1, 0, 1, 0, 0, 1, 1]),
            (PictualElement::White(960), vec![0, 1, 1, 0, 1, 0, 1, 0, 0]),
            (PictualElement::White(1024), vec![0, 1, 1, 0, 1, 0, 1, 0, 1]),
            (PictualElement::White(1088), vec![0, 1, 1, 0, 1, 0, 1, 1, 0]),
            (PictualElement::White(1152), vec![0, 1, 1, 0, 1, 0, 1, 1, 1]),
            (PictualElement::White(1216), vec![0, 1, 1, 0, 1, 1, 0, 0, 0]),
            (PictualElement::White(1280), vec![0, 1, 1, 0, 1, 1, 0, 0, 1]),
            (PictualElement::White(1344), vec![0, 1, 1, 0, 1, 1, 0, 1, 0]),
            (PictualElement::White(1408), vec![0, 1, 1, 0, 1, 1, 0, 1, 1]),
            (PictualElement::White(1472), vec![0, 1, 0, 0, 1, 1, 0, 0, 0]),
            (PictualElement::White(1536), vec![0, 1, 0, 0, 1, 1, 0, 0, 1]),
            (PictualElement::White(1600), vec![0, 1, 0, 0, 1, 1, 0, 1, 0]),
            (PictualElement::White(1664), vec![0, 1, 1, 0, 0, 0]),
            (PictualElement::White(1728), vec![0, 1, 0, 0, 1, 1, 0, 1, 1]),
            (PictualElement::MakeUp(1792), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (PictualElement::MakeUp(1856), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0]),
            (PictualElement::MakeUp(1920), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1]),
            (PictualElement::MakeUp(1984), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0]),
            (PictualElement::MakeUp(2048), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1]),
            (PictualElement::MakeUp(2112), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
            (PictualElement::MakeUp(2176), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1]),
            (PictualElement::MakeUp(2240), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0]),
            (PictualElement::MakeUp(2304), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::MakeUp(2368), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0]),
            (PictualElement::MakeUp(2432), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1]),
            (PictualElement::MakeUp(2496), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0]),
            (PictualElement::MakeUp(2560), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1]),
            (PictualElement::Unused, vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];

    let mut black_codes = vec![
            (PictualElement::Black(0), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (PictualElement::Black(1), vec![0, 1, 0]),
            (PictualElement::Black(2), vec![1, 1]),
            (PictualElement::Black(3), vec![1, 0]),
            (PictualElement::Black(4), vec![0, 1, 1]),
            (PictualElement::Black(5), vec![0, 0, 1, 1]),
            (PictualElement::Black(6), vec![0, 0, 1, 0]),
            (PictualElement::Black(7), vec![0, 0, 0, 1, 1]),
            (PictualElement::Black(8), vec![0, 0, 0, 1, 0, 1]),
            (PictualElement::Black(9), vec![0, 0, 0, 1, 0, 0]),
            (PictualElement::Black(10), vec![0, 0, 0, 0, 1, 0, 0]),
            (PictualElement::Black(11), vec![0, 0, 0, 0, 1, 0, 1]),
            (PictualElement::Black(12), vec![0, 0, 0, 0, 1, 1, 1]),
            (PictualElement::Black(13), vec![0, 0, 0, 0, 0, 1, 0, 0]),
            (PictualElement::Black(14), vec![0, 0, 0, 0, 0, 1, 1, 1]),
            (PictualElement::Black(15), vec![0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (PictualElement::Black(16), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::Black(17), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (PictualElement::Black(18), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (PictualElement::Black(19), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1]),
            (PictualElement::Black(20), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0]),
            (PictualElement::Black(21), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (PictualElement::Black(22), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (PictualElement::Black(23), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0]),
            (PictualElement::Black(24), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::Black(25), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0]),
            (PictualElement::Black(26), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0]),
            (PictualElement::Black(27), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1]),
            (PictualElement::Black(28), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0]),
            (PictualElement::Black(29), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1]),
            (PictualElement::Black(30), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0]),
            (PictualElement::Black(31), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1]),
            (PictualElement::Black(32), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
            (PictualElement::Black(33), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1]),
            (PictualElement::Black(34), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0]),
            (PictualElement::Black(35), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1]),
            (PictualElement::Black(36), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0]),
            (PictualElement::Black(37), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1]),
            (PictualElement::Black(38), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0]),
            (PictualElement::Black(39), vec![0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1]),
            (PictualElement::Black(40), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (PictualElement::Black(41), vec![0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1]),
            (PictualElement::Black(42), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 0]),
            (PictualElement::Black(43), vec![0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1]),
            (PictualElement::Black(44), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0]),
            (PictualElement::Black(45), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
            (PictualElement::Black(46), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0]),
            (PictualElement::Black(47), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1]),
            (PictualElement::Black(48), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0]),
            (PictualElement::Black(49), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1]),
            (PictualElement::Black(50), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0]),
            (PictualElement::Black(51), vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1]),
            (PictualElement::Black(52), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0]),
            (PictualElement::Black(53), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1]),
            (PictualElement::Black(54), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0]),
            (PictualElement::Black(55), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1]),
            (PictualElement::Black(56), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0]),
            (PictualElement::Black(57), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0]),
            (PictualElement::Black(58), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1]),
            (PictualElement::Black(59), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1]),
            (PictualElement::Black(60), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0]),
            (PictualElement::Black(61), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0]),
            (PictualElement::Black(62), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
            (PictualElement::Black(63), vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1]),
            (PictualElement::Black(64), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 1]),
            (PictualElement::Black(128), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0]),
            (PictualElement::Black(192), vec![0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1]),
            (PictualElement::Black(256), vec![0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1]),
            (PictualElement::Black(320), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1]),
            (PictualElement::Black(384), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0]),
            (PictualElement::Black(448), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1]),
            (PictualElement::Black(512), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0]),
            (PictualElement::Black(576), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1]),
            (PictualElement::Black(640), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0]),
            (PictualElement::Black(704), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 1]),
            (PictualElement::Black(768), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0]),
            (PictualElement::Black(832), vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1]),
            (PictualElement::Black(896), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0]),
            (PictualElement::Black(960), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1]),
            (PictualElement::Black(1024), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0]),
            (PictualElement::Black(1088), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1]),
            (PictualElement::Black(1152), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0]),
            (PictualElement::Black(1216), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1]),
            (PictualElement::Black(1280), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0]),
            (PictualElement::Black(1344), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1]),
            (PictualElement::Black(1408), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0]),
            (PictualElement::Black(1472), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
            (PictualElement::Black(1536), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0]),
            (PictualElement::Black(1600), vec![0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1]),
            (PictualElement::Black(1664), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0]),
            (PictualElement::Black(1728), vec![0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1]),
            (PictualElement::MakeUp(1792), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]),
            (PictualElement::MakeUp(1856), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0]),
            (PictualElement::MakeUp(1920), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1]),
            (PictualElement::MakeUp(1984), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0]),
            (PictualElement::MakeUp(2048), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1]),
            (PictualElement::MakeUp(2112), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
            (PictualElement::MakeUp(2176), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1]),
            (PictualElement::MakeUp(2240), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0]),
            (PictualElement::MakeUp(2304), vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1]),
            (PictualElement::MakeUp(2368), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0]),
            (PictualElement::MakeUp(2432), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1]),
            (PictualElement::MakeUp(2496), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0]),
            (PictualElement::MakeUp(2560), vec![0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1]),
            (PictualElement::Unused, vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];
    
    match algo {
        Algorithm::Group3_1D => {
            let len = white_codes.len();
            white_codes[len - 1]=(PictualElement::EOL, vec![0,0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            let len = black_codes.len();
            black_codes[len - 1] = (PictualElement::EOL, vec![0,0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
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
) -> Result<PictualElement> {
    let tree = huffman.get(color);
    let mut bytes = 0;
    loop {
        let pe = reader.read_huffman(tree)?;
        let n = pe.len();
        bytes += n;
        if n < 64 {
            return Ok(PictualElement::from_color(color, bytes));
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

    pub fn is_white(self) -> bool {
        self == Color::White
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

    fn fill(&mut self, pe: PictualElement) {
        let mut pos = self.pos.unwrap_or_default();
        for _ in 0..pe.len() {
            self.cur.set(pos, pe.is_white());
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
                let b1 = self.last.b1(self.pos, self.cur_color.is_white());
                self.fill(PictualElement::from_color(
                    self.cur_color,
                    (b1 as i16 - self.pos.unwrap_or_default() as i16 + n as i16) as u16,
                ));
                self.cur_color = self.cur_color.toggle();
            }
            Code::Pass => {
                let b1 = self.last.b1(self.pos, self.cur_color.is_white());
                let b2 = self.last.next_flip(Some(b1));
                self.fill(PictualElement::from_color(
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
        let image_line: BitVec<u8, Msb0> = repeat(true).take(self.width as usize).collect();
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

#[cfg(test)]
mod tests;
