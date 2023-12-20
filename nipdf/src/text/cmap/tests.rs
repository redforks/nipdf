use super::*;

#[test]
fn test_decode() {
    // Identity-H
    let cmap = CMap::predefined("Identity-H").unwrap();
    let data = vec![0x00, 0x01, 0x02, 0x03];
    let decoded = cmap.decode(&data);
    assert_eq!(decoded, vec![0x0001, 0x0203]);

    // ETen-B5-H
    let cmap = CMap::predefined("ETen-B5-H").unwrap();
    let data = vec![0xA4, 0x61, 0xDC, 0xD1];
    let decoded = cmap.decode(&data);
    // 5141: 兀, 55C1: 嗀
    assert_eq!(decoded, vec![0x5140, 0x55c0]);
}
