use bitstream_io::{
    huffman::{compile_read_tree, ReadHuffmanTree},
    read::{BitRead, BitReader},
    BigEndian, HuffmanRead,
};
use bitvec::{prelude::Msb0, slice::BitSlice, vec::BitVec};
use educe::Educe;
use log::error;
use std::iter::repeat;
use tinyvec::{array_vec, ArrayVec};

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
    EndOfBlock,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Educe)]
#[educe(Default)]
enum PictualElement {
    Black(u16),
    White(u16),
    MakeUp(u16),

    // used in Group3_1D
    EOL,

    #[educe(Default)]
    NotDef,
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

struct RunHuffmanTree {
    black: Box<[ReadHuffmanTree<BigEndian, PictualElement>]>,
    white: Box<[ReadHuffmanTree<BigEndian, PictualElement>]>,
}

impl RunHuffmanTree {
    fn get(&self, color: Color) -> &[ReadHuffmanTree<BigEndian, PictualElement>] {
        match color {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group4Code {
    Pass,
    Horizontal,
    // negative is left, position is right
    Vertical(i8),
    Extension,
    EOFB,

    NotDef,
}

#[rustfmt::skip]
fn build_group4_huffman_tree() -> Box<[ReadHuffmanTree<BigEndian, Group4Code>]> {
    compile_read_tree(vec![
        (Group4Code::Vertical(0),  vec![1]),

        (Group4Code::Horizontal,   vec![0, 0, 1]),
        (Group4Code::Vertical(-1), vec![0, 1, 0]),
        (Group4Code::Vertical(1),  vec![0, 1, 1]),

        (Group4Code::Pass,         vec![0, 0, 0, 1]),

        (Group4Code::Vertical(-2), vec![0, 0, 0, 0, 1, 0]),
        (Group4Code::Vertical(2),  vec![0, 0, 0, 0, 1, 1]),
        (Group4Code::Extension,    vec![0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::Vertical(-3), vec![0, 0, 0, 0, 0, 1, 0]),
        (Group4Code::Vertical(3),  vec![0, 0, 0, 0, 0, 1, 1]),
        (Group4Code::EOFB,         vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::NotDef,       vec![0, 0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::NotDef,       vec![0, 0, 0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::NotDef,       vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::NotDef,       vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        (Group4Code::NotDef,       vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    ])
    .unwrap()
}

#[rustfmt::skip]
fn build_run_huffman(algo: Algorithm) -> RunHuffmanTree {
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
            (PictualElement::NotDef, vec![0, 0, 0, 0, 0, 0, 0, 0]),
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
            (PictualElement::NotDef, vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];
    
    match algo {
        Algorithm::Group3_1D => {
            let len = white_codes.len();
            white_codes[len - 1]=(PictualElement::EOL, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            let len = black_codes.len();
            black_codes[len - 1] = (PictualElement::EOL, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        }
        Algorithm::Group3_2D(_) => todo!(),
        Algorithm::Group4 => { },
    }

    RunHuffmanTree {
        white: compile_read_tree(white_codes).unwrap(),
        black: compile_read_tree(black_codes).unwrap(),
    }
}

fn next_run(
    reader: &mut impl HuffmanRead<BigEndian>,
    huffman: &RunHuffmanTree,
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

trait CodeIterator {
    fn next_code(&self, reader: &mut BitReader<&[u8], BigEndian>, state: State) -> Result<Code>;
}

struct Group4CodeIterator {
    huffman: RunHuffmanTree,
    group4_huffman: Box<[ReadHuffmanTree<BigEndian, Group4Code>]>,
    flags: Flags,
}

impl Group4CodeIterator {
    fn new(flags: Flags) -> Self {
        Self {
            huffman: build_run_huffman(Algorithm::Group4),
            group4_huffman: build_group4_huffman_tree(),
            flags,
        }
    }
}

impl CodeIterator for Group4CodeIterator {
    fn next_code(&self, reader: &mut BitReader<&[u8], BigEndian>, state: State) -> Result<Code> {
        if self.flags.encoded_byte_align && state.is_new_line {
            reader.byte_align();
        }

        match reader.read_huffman(&self.group4_huffman)? {
            Group4Code::Pass => return Ok(Code::Pass),
            Group4Code::Horizontal => {
                let a0a1 = next_run(reader, &self.huffman, state.color)?;
                let a1a2 = next_run(reader, &self.huffman, state.color.toggle())?;
                return Ok(Code::Horizontal(a0a1, a1a2));
            }
            Group4Code::Vertical(n) => {
                return Ok(Code::Vertical(n));
            }
            Group4Code::EOFB => {
                assert_eq!(reader.read_huffman(&self.group4_huffman)?, Group4Code::EOFB);
                return Ok(Code::EndOfBlock);
            }
            Group4Code::Extension => {
                return Ok(Code::Extension(reader.read(3)?));
            }
            Group4Code::NotDef => {
                return Err(DecodeError::InvalidCode);
            }
        }
    }
}

fn iter_code<I: CodeIterator + 'static>(
    buf: &[u8],
    i: I,
) -> impl FnMut(State) -> Option<Result<Code>> + '_ {
    let mut reader = BitReader::endian(buf, BigEndian);
    move |state| match i.next_code(&mut reader, state) {
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

type PictualElementArray = [PictualElement; 2];
type PictualElementVec = ArrayVec<PictualElementArray>;

trait CodeDecoder {
    fn decode(
        &self,
        last: &LineBuf,
        cur_color: Color,
        cur_pos: Option<usize>,
        code: Code,
    ) -> Result<(Color, PictualElementVec)>;
}

#[derive(Copy, Clone)]
struct Group4CodeDecoder;

impl CodeDecoder for Group4CodeDecoder {
    fn decode(
        &self,
        last: &LineBuf,
        cur_color: Color,
        cur_pos: Option<usize>,
        code: Code,
    ) -> Result<(Color, PictualElementVec)> {
        match code {
            Code::Horizontal(a0a1, a1a2) => {
                Ok((cur_color, array_vec!(PictualElementArray => a0a1, a1a2)))
            }
            Code::Vertical(n) => {
                let b1 = last.b1(cur_pos, cur_color.is_white());
                let pe = array_vec!(PictualElementArray=> PictualElement::from_color(
                    cur_color,
                    (b1 as i16 - cur_pos.unwrap_or_default() as i16 + n as i16) as u16,
                ));
                Ok((cur_color.toggle(), pe))
            }
            Code::Pass => {
                let b1 = last.b1(cur_pos, cur_color.is_white());
                let b2 = last.next_flip(Some(b1));
                let pe = array_vec!(PictualElementArray =>  PictualElement::from_color(
                    cur_color,
                    (b2 - cur_pos.unwrap_or_default()) as u16,
                ));
                Ok((cur_color, pe))
            }
            _ => unreachable!(),
        }
    }
}

struct LineDecoder<'a> {
    last: LineBuf<'a>,
    cur: BitVec<u8, Msb0>,
    cur_color: Color,
    pos: Option<usize>,
}

impl<'a> LineDecoder<'a> {
    fn new(last: &'a BitSlice<u8, Msb0>, cur: BitVec<u8, Msb0>) -> Self {
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
        State {
            color: self.cur_color,
            is_new_line: self.is_new_line(),
        }
    }

    // return true if current line filled.
    pub fn decode(&mut self, code_decoder: impl CodeDecoder, code: Code) -> Result<bool> {
        let (color, pe) = code_decoder.decode(&self.last, self.cur_color, self.pos, code)?;
        self.cur_color = color;
        for pe in pe {
            self.fill(pe);
        }
        debug_assert!(self.pos.unwrap_or_default() <= self.cur.len());
        Ok(self.pos.unwrap_or_default() == self.cur.len())
    }

    pub fn take(self) -> BitVec<u8, Msb0> {
        self.cur
    }
}

#[derive(Debug, Educe, Copy, Clone)]
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
    fn do_decode(
        &self,
        code_decoder: impl CodeDecoder + Copy + 'static,
        code_iterator: impl CodeIterator + 'static,
        buf: &[u8],
    ) -> Result<BitVec<u8, Msb0>> {
        let mut r = BitVec::<u8, Msb0>::with_capacity(
            self.rows.unwrap_or(30) as usize * self.width as usize,
        );
        let imagnation_line: BitVec<u8, Msb0> = repeat(true).take(self.width as usize).collect();
        let mut line_decoder = LineDecoder::new(
            &imagnation_line[..],
            repeat(true).take(self.width as usize).collect(),
        );
        let mut next_code = iter_code(buf, code_iterator);
        loop {
            let Some(code) = next_code(line_decoder.state()) else {
                break;
            };

            match code? {
                Code::Extension(_) => todo!(),
                Code::EndOfBlock => break,
                code => {
                    if line_decoder.decode(code_decoder, code)? {
                        let line = line_decoder.take();
                        r.extend(&line);
                        if !self.flags.end_of_block {
                            if let Some(rows) = self.rows {
                                if rows as usize == r.len() / self.width as usize {
                                    break;
                                }
                            }
                        }
                        line_decoder = LineDecoder::new(&r[r.len() - self.width as usize..], line);
                    }
                }
            }
        }
        Ok(r)
    }

    pub fn decode(&self, buf: &[u8]) -> Result<Vec<u8>> {
        assert!(!matches!(self.algorithm, Algorithm::Group3_2D(_)));

        let mut r = self.do_decode(Group4CodeDecoder, Group4CodeIterator::new(self.flags), buf)?;
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
