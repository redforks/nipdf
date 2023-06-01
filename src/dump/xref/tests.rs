use super::*;

#[test]
fn xref_entry_dumper() {
    // Free
    assert_eq!(
        format!("{}", XrefEntryDumper(100, &XrefEntry::Free)),
        "100: free"
    );

    // unusable free
    assert_eq!(
        format!("{}", XrefEntryDumper(101, &XrefEntry::UnusableFree)),
        "101: unusable free"
    );

    // normal
    assert_eq!(
        format!(
            "{}",
            XrefEntryDumper(
                102,
                &XrefEntry::Normal {
                    offset: 1000,
                    generation: 12
                }
            )
        ),
        "102: normal 1000 12"
    );

    // compressed
    assert_eq!(
        format!(
            "{}",
            XrefEntryDumper(
                103,
                &XrefEntry::Compressed {
                    container: 1000,
                    index: 12
                }
            )
        ),
        "103: compressed 1000 12"
    );
}
