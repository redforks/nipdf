pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(feature = "dump")]
pub mod dump;
pub mod file;
pub mod object;
pub mod old_object;
pub mod parser;
