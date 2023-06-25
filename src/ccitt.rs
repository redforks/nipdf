use bitstream_io::{
    huffman::{compile_read_tree, ReadHuffmanTree},
    read::{BitRead, BitReader},
    BigEndian, HuffmanRead,
};
use std::{
    collections::{HashMap, HashSet},
    iter::{from_fn, repeat},
};

use itertools::Itertools;
use once_cell::unsync::Lazy;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Code {
    Pass,
    Horizontal(Run, Run), // a0a1, a1a2
    Vertical(i8),
    Extension,
    EndOfFassimileBlock,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Run {
    color: u8,
    bytes: u16,
}

impl Run {
    fn new(color: u8, bytes: u16) -> Self {
        Self { color, bytes }
    }
}

const BLACK: u8 = 0;
const WHITE: u8 = 255;
const GRAY: u8 = 128;
const NOT_USED: u8 = 100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IOError: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Horizontal run color mismatch")]
    HorizontalRunColorMismatch,
}

type Result<T> = std::result::Result<T, Error>;

struct RunHuffamnTree {
    black: Box<[ReadHuffmanTree<BigEndian, Run>]>,
    white: Box<[ReadHuffmanTree<BigEndian, Run>]>,
}

impl RunHuffamnTree {
    fn get(&self, color: u8) -> &Box<[ReadHuffmanTree<BigEndian, Run>]> {
        match color {
            BLACK => &self.black,
            WHITE => &self.white,
            _ => unreachable!(),
        }
    }
}

#[rustfmt::skip]
fn build_run_huffman() -> RunHuffamnTree {
    RunHuffamnTree {
        white: compile_read_tree(vec![
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
        ])
        .unwrap(),
        black: compile_read_tree(vec![
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
        ])
        .unwrap(),
    }
}

fn next_run(
    reader: &mut impl HuffmanRead<BigEndian>,
    huffman: &RunHuffamnTree,
    color: u8,
) -> Result<Run> {
    let tree = huffman.get(color);
    let mut r = Run::new(color, 0);
    loop {
        let run = reader.read_huffman(tree)?;
        match run.color {
            GRAY => {}
            BLACK | WHITE => {
                if run.color != color {
                    return Err(Error::HorizontalRunColorMismatch);
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

fn iter_code(buf: &[u8]) -> impl FnMut(u8) -> Option<Result<Code>> + '_ {
    let huffman = build_run_huffman();
    fn next(
        huffman: &RunHuffamnTree,
        reader: &mut (impl BitRead + HuffmanRead<BigEndian>),
        hor_color: u8,
    ) -> Result<Code> {
        if reader.read_bit()? {
            // 1
            return Ok(Code::Vertical(0));
        }

        match reader.read::<u8>(2)? {
            0b11 => Ok(Code::Vertical(1)),  // 011
            0b10 => Ok(Code::Vertical(-1)), // 010
            0b01 => {
                let a0a1 = next_run(reader, &huffman, hor_color)?;
                let a1a2 = next_run(reader, &huffman, neg_color(hor_color))?;
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
                            // 000001
                            true => Ok(Code::Vertical(3)),   // 0000011
                            false => Ok(Code::Vertical(-3)), // 0000010
                        },
                        0b00 => {
                            if reader.read::<u8>(3)? == 0b010 {
                                // 000000010
                                unimplemented!("Extension code")
                                // Ok(Code::Extension)
                            } else {
                                todo!()
                            }
                        }
                        0b10 => Ok(Code::Vertical(-2)), // 000010
                        _ => unreachable!(),
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    let mut reader = BitReader::endian(buf, BigEndian);
    move |hor_color| match next(&huffman, &mut reader, hor_color) {
        Ok(v) => Some(Ok(v)),
        Err(e) => match e {
            Error::IOError(io_err) => {
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

struct LineBuf<'a>(&'a [u8]);

impl<'a> LineBuf<'a> {
    fn b1(&self, pos: usize, pos_color: u8) -> Option<usize> {
        todo!()
    }

    fn next_flip(&self, pos: usize) -> Option<usize> {
        todo!()
    }
}

struct Coder<'a> {
    last: LineBuf<'a>,
    cur: &'a mut [u8],
    cur_color: Option<u8>,
    pos: usize,
}

fn neg_color(c: u8) -> u8 {
    match c {
        BLACK => WHITE,
        WHITE => BLACK,
        _ => unreachable!(),
    }
}

impl<'a> Coder<'a> {
    fn new(last: &'a [u8], cur: &'a mut [u8]) -> Self {
        debug_assert!(last.len() == cur.len());
        Self {
            last: LineBuf(last),
            cur,
            cur_color: None,
            pos: 0,
        }
    }

    fn fill(&mut self, run: Run) {
        for _ in 0..run.bytes {
            self.cur[self.pos] = run.color;
            self.pos += 1;
        }
    }

    // return true if current line filled.
    fn decode(&mut self, code: Code) -> Result<bool> {
        match code {
            Code::Horizontal(a0a1, a1a2) => {
                self.fill(a0a1);
                self.fill(a1a2);
            }
            Code::Vertical(n) => {
                let color = self.cur_color.expect("cur_color exist in vertical");
                let b1 = self
                    .last
                    .b1(self.pos, color)
                    .expect("b1 should exist in vertical");
                self.fill(Run::new(
                    color,
                    (b1 as i16 - self.pos as i16 + n as i16) as u16,
                ));
                if n < 0 {
                    self.fill(Run::new(neg_color(color), -n as u16));
                }
                self.pos = b1;
            }
            Code::Pass => {
                let color = neg_color(self.last.0[self.pos]);
                let b1 = self
                    .last
                    .b1(self.pos, color)
                    .expect("b1 should exist in pass");
                let b2 = self.last.next_flip(b1).expect("b2 not exist in pass");
                self.fill(Run::new(color, (b2 - self.pos) as u16));
                self.pos = b2;
            }
            _ => unreachable!(),
        };
        debug_assert!(self.pos <= self.cur.len());
        Ok(self.pos == self.cur.len())
    }

    fn last_line_color(&self) -> u8 {
        self.last.0[self.pos]
    }
}

pub fn decode(buf: &[u8], width: u16, rows: Option<usize>) -> Result<Vec<u8>> {
    let image_line = repeat(WHITE).take(width as usize).collect_vec();
    let last_line = &image_line[..];
    let mut r = Vec::with_capacity(rows.unwrap_or(30) * width as usize);
    let mut line_buf = repeat(WHITE).take(width as usize).collect_vec();
    let mut next_code = iter_code(buf);
    loop {
        let mut coder = Coder::new(last_line, &mut line_buf);
        match next_code(coder.last_line_color()) {
            None => break,
            Some(code) => match code? {
                Code::Extension => todo!(),
                Code::EndOfFassimileBlock => {
                    todo!()
                }
                code => {
                    if coder.decode(code)? {
                        r.extend_from_slice(&line_buf[..]);

                        #[cfg(test)]
                        line_buf.fill(0x1f);

                        coder = Coder::new(&r[r.len() - width as usize..], &mut line_buf);
                    }
                }
            },
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests;
