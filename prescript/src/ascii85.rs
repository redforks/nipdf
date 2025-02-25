//! Copied from <https://github.com/JoNil/ascii85>, because the original crate returns error `Box<dyn Error>`
//! We require a error that implements [Sync] and [Send].
const TABLE: [u32; 5] = [85 * 85 * 85 * 85, 85 * 85 * 85, 85 * 85, 85, 1];

fn decode_digit(digit: u8, counter: &mut usize, chunk: &mut u32, result: &mut Vec<u8>) {
    let byte = digit - 33;

    *chunk += byte as u32 * TABLE[*counter];

    if *counter == 4 {
        result.extend_from_slice(&chunk.to_be_bytes());
        *chunk = 0;
        *counter = 0;
    } else {
        *counter += 1;
    }
}

use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum Ascii85Error {
    #[snafu(display("Misaligned 'z' in input"))]
    MisalignedZ,
    #[snafu(display("Input char '{}' is out of range for Ascii85", digit))]
    CharOutOfRange { digit: u8 },
}

/// On error, return the partial result and the error.
pub fn decode(input: &[u8]) -> Result<Vec<u8>, (Vec<u8>, Ascii85Error)> {
    let mut result = Vec::with_capacity(4 * (input.len() / 5 + 16));

    let mut counter = 0;
    let mut chunk = 0;

    // Find start of actual data by trimming whitespace and "<~" prefix
    let mut start_idx = 0;
    while start_idx < input.len() && input[start_idx].is_ascii_whitespace() {
        start_idx += 1;
    }
    if start_idx + 2 <= input.len() && input[start_idx..start_idx + 2] == [b'<', b'~'] {
        start_idx += 2;
    }

    // Find end of data by trimming whitespace and "~>" suffix
    let mut end_idx = input.len();
    while end_idx > start_idx && input[end_idx - 1].is_ascii_whitespace() {
        end_idx -= 1;
    }
    // Trim end '~>', '~' and '>' maybe separate by whitespace (include end of line)
    if end_idx >= 2 && input[end_idx - 1] == b'>' {
        let original_end_idx = end_idx;
        end_idx -= 1;
        while end_idx > start_idx && input[end_idx - 1].is_ascii_whitespace() {
            end_idx -= 1;
        }
        if end_idx >= 1 && input[end_idx - 1] == b'~' {
            end_idx -= 1;
        } else {
            end_idx = original_end_idx;
        }
    }

    for &digit in input[start_idx..end_idx]
        .iter()
        .filter(|&&c| !c.is_ascii_whitespace())
    {
        if digit == b'z' {
            if counter == 0 {
                result.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            } else {
                return Err((result, Ascii85Error::MisalignedZ));
            }
        }

        if !(33..=117).contains(&digit) {
            return Err((result, Ascii85Error::CharOutOfRange { digit }));
        }

        decode_digit(digit, &mut counter, &mut chunk, &mut result);
    }

    let mut to_remove = 0;

    while counter != 0 {
        decode_digit(b'u', &mut counter, &mut chunk, &mut result);
        to_remove += 1;
    }

    result.drain((result.len() - to_remove)..result.len());

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decode_test() {
        assert_eq!(
            decode(b"<~9jqo^F*2M7/c~>").unwrap(),
            [77, 97, 110, 32, 115, 117, 114, 101, 46]
        );

        assert!(
            decode(br#"
                <~9jqo^BlbD-BleB1DJ+*+F(f,q/0JhKF<GL>Cj@.4Gp$d7F!,L7@<6@)/0JDEF<G%<+EV:2F!,
                O<DJ+*.@<*K0@<6L(Df-\0Ec5e;DffZ(EZee.Bl.9pF"AGXBPCsi+DGm>@3BB/F*&OCAfu2/AKY
                i(DIb:@FD,*)+C]U=@3BN#EcYf8ATD3s@q?d$AftVqCh[NqF<G:8+EV:.+Cf>-FD5W8ARlolDIa
                l(DId<j@<?3r@:F%a+D58'ATD4$Bl@l3De:,-DJs`8ARoFb/0JMK@qB4^F!,R<AKZ&-DfTqBG%G
                >uD.RTpAKYo'+CT/5+Cei#DII?(E,9)oF*2M7/c~>"#
            ).unwrap().iter().zip(
                b"Man is distinguished, not only by his reason, but by this singular passion from other animals, which is a lust of the mind, that by a perseverance of delight in the continued and indefatigable generation of knowledge, exceeds the short vehemence of any carnal pleasure."
                    .iter()
            ).all(|(a, b)| a == b)
        );

        assert_eq!(
            decode(&[b'<', b'~', 47, 99, 117, 117, 117, b'~', b'>']).unwrap(),
            [46, 3, 25, 180]
        );
    }

    #[test]
    fn decode_for_pdf() {
        // extract from sample_files/normal/ASCII85_RunLengthDecode.pdf object 8
        let buf = include_bytes!("./ascii85-1");
        insta::assert_debug_snapshot!(decode(buf).unwrap());
    }

    #[test]
    fn decode_for_pdf2() {
        // extract from sample_files/normal/T-REC-T.4-199904-S!!PDF-E.pdf object 394
        let buf =
            b"8;Z][_$pAe#R%BbR?D]?kQDgNR._&7O9nQ2[sQV\\'@1UnqbR1B'u^7Tz%uWSlp2]mC6\\#1;4[o%3~>";
        insta::assert_debug_snapshot!(decode(buf).unwrap());
    }
}
