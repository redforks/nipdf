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

#[test]
fn option_dumper() {
    assert_eq!("3", format!("{}", OptionDumper::new(&Some(3))));
    assert_eq!("None", format!("{}", OptionDumper::new(&None::<i32>)));
    assert_eq!(
        "[3, 4]",
        format!("{:?}", OptionDumper::new(&Some(vec![3, 4])))
    );
    assert_eq!("None", format!("{:?}", OptionDumper::new(&None::<i32>)));
}
