use super::*;

#[test]
fn pdf_func() {
    // two-in, one-out
    let script = b"{ add }";
    let func = PdfFunc::new(script.as_slice(), 1);
    let r = func.exec(&[1.0, 2.0]).unwrap();
    assert_eq!(r, vec![3.0].into_boxed_slice());

    // two-in, two-out
    let script = b"{ sub 2 }";
    let func = PdfFunc::new(script.as_slice(), 2);
    let r = func.exec(&[1.0, 2.0]).unwrap();
    assert_eq!(r, vec![2.0, 1.0].into_boxed_slice());
}
