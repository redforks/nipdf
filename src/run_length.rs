pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(data.len());
    let d = data;
    let mut c = 0;

    while c < data.len() {
        let length = d[c]; // length is first byte
        if length < 128 {
            let start = c + 1;
            let end = start + length as usize + 1;
            // copy _following_ length + 1 bytes literally
            buf.extend_from_slice(&d[start..end]);
            c = end; // move cursor to next run
        } else if length >= 129 {
            let copy = 257 - length as usize; // copy 2 - 128 times
            let b = d[c + 1]; // copied byte
            buf.extend(std::iter::repeat(b).take(copy));
            c = c + 2; // move cursor to next run
        } else {
            break; // EOD
        }
    }

    buf
}
