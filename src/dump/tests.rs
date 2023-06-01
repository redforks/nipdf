use super::*;

#[test]
fn indent_display() {
    assert_eq!(format!("{}", Indent(0)), "");
    assert_eq!(format!("{}", Indent(1)), "  ");
    assert_eq!(format!("{}", Indent(2)), "    ");
}

#[test]
fn indent_inc() {
    assert_eq!(Indent(0).inc(), Indent(1));
    assert_eq!(Indent(1).inc(), Indent(2));
    assert_eq!(Indent(2).inc(), Indent(3));
}