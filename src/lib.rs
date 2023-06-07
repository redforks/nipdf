pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(feature = "dump")]
pub mod dump;
pub mod object;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
