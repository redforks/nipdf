use super::*;

#[test]
fn compare_char_code() {
    assert!(CharCode::One(100) < CharCode::Two(0, 0));
    assert!(CharCode::Two(100, 200) < CharCode::Three(0, 0, 0));
    assert!(CharCode::One(10) < CharCode::One(100));
    assert!(CharCode::Two(10, 10) < CharCode::Two(10, 100));
    assert!(CharCode::Two(10, 100) < CharCode::Two(11, 0));
    assert!(CharCode::Three(10, 10, 10) < CharCode::Three(10, 10, 100));
    assert!(CharCode::Three(10, 10, 10) < CharCode::Three(11, 0, 0));
}
