use bitreader::{BitReader, BitReaderError};
use educe::Educe;
use image::{GrayImage, ImageBuffer};
use std::collections::HashMap;

#[derive(Debug, Educe)]
#[educe(PartialEq)]
pub enum Jbig2Error {
    InvalidData,
    UnsupportedFeature,
    BitReaderError(BitReaderError),
    ImageError(#[educe(PartialEq(ignore))] image::ImageError),
    UnexpectedEndOfFile,
    Other(String),
    EndOfFile,
}

impl From<BitReaderError> for Jbig2Error {
    fn from(err: BitReaderError) -> Self {
        Jbig2Error::BitReaderError(err)
    }
}

impl From<image::ImageError> for Jbig2Error {
    fn from(err: image::ImageError) -> Self {
        Jbig2Error::ImageError(err)
    }
}

// Represents a bitmap image
#[derive(Debug, Clone)]
struct BitMap {
    width: u32,
    height: u32,
    data: Vec<bool>, // Raw pixel data (1 bit per pixel, packed into bytes)
}

fn read_variable_length_integer(reader: &mut BitReader<'_>) -> Result<u32, Jbig2Error> {
    let mut result: u32 = 0;
    for i in 0..4 {
        let byte = reader.read_u8(8)? as u32;
        result = (result << 7) | (byte & 0x7f);
        if (byte & 0x80) == 0 {
            return Ok(result);
        }
    }
    Err(Jbig2Error::InvalidData) // If we reach here, the VLC is too long/invalid
}

#[derive(Debug, Clone, Copy)]
enum MMRCode {
    Pass,
    Horizontal(usize, usize),
    Vertical(i32),
    Extension,
    Invalid,
    EOL,
}

fn read_mmr_code(reader: &mut BitReader<'_>) -> Result<MMRCode, Jbig2Error> {
    let code = match reader.read_bool() {
        Ok(false) => {
            // Vertical modes
            match reader.read_bits(2) {
                Ok(0) => MMRCode::Vertical(-3),
                Ok(1) => MMRCode::Vertical(-2),
                Ok(2) => MMRCode::Vertical(-1),
                Ok(3) => MMRCode::Vertical(0),
                _ => return Err(Jbig2Error::InvalidData),
            }
        }
        Ok(true) => {
            match reader.read_bool() {
                Ok(false) => {
                    // Horizontal mode
                    let b1 = match reader.read_unary() {
                        Ok(x) => x,
                        Err(_) => return Ok(MMRCode::EOL),
                    };
                    let b2 = match reader.read_unary() {
                        Ok(x) => x,
                        Err(_) => return Ok(MMRCode::EOL),
                    };
                    MMRCode::Horizontal(b1 as usize, b2 as usize)
                }
                Ok(true) => match reader.read_bool() {
                    Ok(false) => match reader.read_bits(1) {
                        Ok(0) => MMRCode::Vertical(1),
                        Ok(1) => MMRCode::Vertical(2),
                        _ => return Err(Jbig2Error::InvalidData),
                    },
                    Ok(true) => match reader.read_bits(3) {
                        Ok(0) => MMRCode::Vertical(3),
                        Ok(1) => MMRCode::Horizontal(0, 0),
                        Ok(2) => MMRCode::Pass,
                        Ok(3) => MMRCode::Horizontal(1, 0),
                        Ok(4) => MMRCode::Horizontal(0, 1),
                        Ok(5) => MMRCode::Horizontal(1, 1),
                        Ok(6) => MMRCode::Horizontal(2, 2),
                        Ok(7) => MMRCode::Horizontal(3, 3),
                        _ => return Err(Jbig2Error::InvalidData),
                    },
                    Err(_) => return Err(Jbig2Error::InvalidData),
                },
                Err(_) => return Err(Jbig2Error::InvalidData),
            }
        }
        Err(_) => return Err(Jbig2Error::InvalidData),
    };
    Ok(code)
}

trait BitReaderExt {
    fn read_bits(&mut self, count: u8) -> Result<u32, BitReaderError>;
    fn read_unary(&mut self) -> Result<u32, BitReaderError>;
}

impl<'a> BitReaderExt for BitReader<'a> {
    fn read_bits(&mut self, count: u8) -> Result<u32, BitReaderError> {
        self.read_u32(count)
    }

    fn read_unary(&mut self) -> Result<u32, BitReaderError> {
        let mut count = 0;
        loop {
            match self.read_bits(1) {
                Ok(0) => return Ok(count),
                Ok(_) => count += 1,
                Err(e) => return Err(e),
            }
        }
    }
}

fn decode_mmr(
    reader: &mut BitReader<'_>,
    width: u32,
    height: u32,
) -> Result<Vec<bool>, Jbig2Error> {
    println!("Decoding MMR data (width: {}, height: {})", width, height);

    let width_usize = width as usize;
    let height_usize = height as usize;

    let mut decoded_data: Vec<bool> = vec![false; width_usize * height_usize];
    let mut reference_row: Vec<bool> = vec![false; width_usize];
    let mut current_row: Vec<bool> = vec![false; width_usize];

    for row in 0..height_usize {
        let mut column = 0;
        while column < width_usize {
            let code = read_mmr_code(reader)?;
            match code {
                MMRCode::Pass => {
                    while column < width_usize && !reference_row[column] {
                        column += 1;
                    }
                    if column < width_usize {
                        column += 1;
                    }
                }
                MMRCode::Horizontal(run1, run2) => {
                    let a0_color = if column > 0 {
                        current_row[column - 1]
                    } else {
                        false
                    };

                    for i in 0..run1 {
                        if column + i < width_usize {
                            current_row[column + i] = !a0_color;
                        }
                    }

                    column += run1;

                    let a0_color = !a0_color;

                    for i in 0..run2 {
                        if column + i < width_usize {
                            current_row[column + i] = !a0_color;
                        }
                    }

                    column += run2;
                }
                MMRCode::Vertical(offset) => {
                    let new_col = (column as i32 + offset) as usize;
                    if new_col < width_usize {
                        current_row[column] = reference_row[new_col];
                    }
                    column += 1;
                }
                MMRCode::Extension => {
                    todo!("Extension MMR code not supported.");
                }
                MMRCode::Invalid => {
                    return Err(Jbig2Error::InvalidData);
                }
                MMRCode::EOL => {
                    break;
                }
            }
        }

        for col in 0..width_usize {
            decoded_data[row * width_usize + col] = current_row[col];
        }

        std::mem::swap(&mut current_row, &mut reference_row);
        current_row.fill(false);
    }

    Ok(decoded_data)
}

struct ArithmeticDecoder {}

impl ArithmeticDecoder {
    fn new() -> Self {
        ArithmeticDecoder {}
    }

    fn decode(
        &mut self,
        reader: &mut BitReader<'_>,
        width: u32,
        height: u32,
    ) -> Result<Vec<bool>, Jbig2Error> {
        println!(
            "Decoding arithmetic coded data (width: {}, height: {})",
            width, height
        );
        // Placeholder implementation. Replace with actual arithmetic decoding logic.
        let total_pixels = width * height;
        let data = vec![false; total_pixels as usize];
        Ok(data)
    }
}

fn parse_symbol_dictionary_segment(
    reader: &mut BitReader<'_>,
    segment_flags: u8,
    segment_data_length: u32,
) -> Result<Vec<BitMap>, Jbig2Error> {
    println!(
        "Parsing Symbol Dictionary Segment (length: {})",
        segment_data_length
    );

    // Determine if MMR encoding is used
    let use_mmr = (segment_flags & 0x01) != 0; //Flag 0 indicates if mmr is used.

    // 1. Number of Exported Symbols
    let num_exported_symbols = read_variable_length_integer(reader)?;
    println!("Number of Exported Symbols: {}", num_exported_symbols);

    // 2. Symbol Width and Height Defaults
    let symbol_width_default = reader.read_u8(8)? as u32;
    let symbol_height_default = reader.read_u8(8)? as u32;
    println!(
        "Symbol Width Default: {}, Symbol Height Default: {}",
        symbol_width_default, symbol_height_default
    );

    // 3. Number of Symbols
    let num_symbols = read_variable_length_integer(reader)?;
    println!("Number of Symbols: {}", num_symbols);

    let mut symbols: Vec<BitMap> = Vec::new();

    // 4. Symbol Data (for now, assume uncompressed bitmaps)
    for i in 0..num_symbols {
        println!("Parsing Symbol {}", i);

        //For now, assume that each symbol's with and height match default value
        let width = symbol_width_default;
        let height = symbol_height_default;

        let data: Vec<bool> = if use_mmr {
            // MMR encoded
            decode_mmr(reader, width, height)?
        } else {
            // Uncompressed
            let total_pixels = width * height;
            let mut data: Vec<bool> = Vec::new();
            let bytes_per_row = (width + 7) / 8;
            for _ in 0..height {
                for _ in 0..bytes_per_row {
                    let byte = reader.read_u8(8)?;
                    for i in 0..8 {
                        if data.len() < total_pixels as usize {
                            data.push((byte >> (7 - i)) & 0x01 != 0);
                        }
                    }
                }
            }

            data
        };

        symbols.push(BitMap {
            width,
            height,
            data,
        });
    }

    Ok(symbols)
}

fn parse_image_region_segment(
    reader: &mut BitReader<'_>,
    segment_flags: u8,
    segment_data_length: u32,
    symbol_dictionaries: &HashMap<u32, Vec<BitMap>>,
) -> Result<BitMap, Jbig2Error> {
    println!(
        "Parsing Image Region Segment (length: {})",
        segment_data_length
    );

    // 1. Read the Image Region segment header parameters
    let x_resolution = read_variable_length_integer(reader)?;
    let y_resolution = read_variable_length_integer(reader)?;
    let image_width = read_variable_length_integer(reader)?;
    let image_height = read_variable_length_integer(reader)?;

    println!(
        "Image Region Dimensions: width={}, height={}, x_resolution={}, y_resolution={}",
        image_width, image_height, x_resolution, y_resolution
    );

    // 2. Read the Image Region flags
    let image_region_flags = reader.read_u8(8)?;
    println!("Image Region Flags: 0x{:02X}", image_region_flags);

    // 3. Parse the Image Region flags
    let coding_method = image_region_flags & 0x03; // Bits 0-1
    let combination_operator = (image_region_flags >> 2) & 0x01; // Bit 2
    let exported = (image_region_flags >> 3) & 0x01; // Bit 3
    let num_symbol_refs = (image_region_flags >> 4) & 0x0F; // Bits 4-7

    println!("Coding Method: {}", coding_method);
    println!("Combination Operator: {}", combination_operator);
    println!("Exported: {}", exported);
    println!("Number of Symbol References: {}", num_symbol_refs);

    // 4. Read the segment numbers of the referenced Symbol Dictionaries
    let mut referenced_symbol_dictionaries: Vec<&Vec<BitMap>> = Vec::new();
    for _ in 0..num_symbol_refs {
        let referenced_segment_number = read_variable_length_integer(reader)?;
        println!("Referenced Segment Number: {}", referenced_segment_number);

        // Look up the symbol dictionary in the HashMap
        match symbol_dictionaries.get(&referenced_segment_number) {
            Some(symbols) => {
                referenced_symbol_dictionaries.push(symbols);
            }
            None => {
                return Err(Jbig2Error::InvalidData); // Referenced segment not found
            }
        }
    }

    // 5. Decode the Image Region data based on the coding method
    let image_data: Vec<bool> = match coding_method {
        0 => {
            todo!("Symbol refinement not supported yet");
        }
        1 => {
            todo!("Template matching not supported yet");
        }
        2 => {
            // Arithmetic coding
            let mut arithmetic_decoder = ArithmeticDecoder::new();
            arithmetic_decoder.decode(reader, image_width, image_height)?
        }
        _ => {
            return Err(Jbig2Error::UnsupportedFeature);
        }
    };

    //For now, just create a dummy bitmap
    Ok(BitMap {
        width: image_width,
        height: image_height,
        data: image_data,
    })
}

pub fn decode_jbig2(data: &[u8]) -> Result<GrayImage, Jbig2Error> {
    println!("Decoding JBIG2 data (length: {} bytes)", data.len());

    if data.is_empty() {
        return Err(Jbig2Error::InvalidData); // File too short
    }

    let mut reader = BitReader::new(data);
    let mut segment_number: u32 = 0; // Keep track of the current segment number
    let mut symbol_dictionaries: HashMap<u32, Vec<BitMap>> = HashMap::new(); // Store symbol dictionaries

    loop {
        // 1. Read the segment number
        let delta_segment_number = match read_variable_length_integer(&mut reader) {
            Ok(num) => num,
            Err(Jbig2Error::BitReaderError(BitReaderError::NotEnoughData { .. })) => {
                //Expected end of stream
                println!("End of stream reached");
                break;
            }
            Err(e) => return Err(e),
        };

        if delta_segment_number == 0 {
            //End of file segment
            println!("End of file");
            break;
        }

        segment_number += delta_segment_number;
        println!("Segment Number: {}", segment_number);

        // 2. Read the segment flags (1 byte)
        let segment_flags = reader.read_u8(8)?;
        println!("Segment Flags: 0x{:02X}", segment_flags);

        // 3. Determine the segment type based on the flags (we'll expand on this later)
        let segment_type = segment_flags & 0x3f; // Lower 6 bits determine the type
        println!("Segment Type: {}", segment_type);

        // 4. Read the segment data length (3 bytes if flag 6 is 0, otherwise variable)
        let segment_data_length: u32;
        if (segment_flags & 0x40) == 0 {
            // 3-byte length
            segment_data_length = reader.read_u32(24)?;
        } else {
            // Variable length
            segment_data_length = read_variable_length_integer(&mut reader)?;
        }
        println!("Segment Data Length: {}", segment_data_length);

        // 5. Read the segment data
        match segment_type {
            0 => {
                // Symbol Dictionary Segment
                let symbols = parse_symbol_dictionary_segment(
                    &mut reader,
                    segment_flags,
                    segment_data_length,
                )?;
                symbol_dictionaries.insert(segment_number, symbols);
            }
            6 => {
                // Image Region Segment
                let image_region = parse_image_region_segment(
                    &mut reader,
                    segment_flags,
                    segment_data_length,
                    &symbol_dictionaries,
                )?;
            }
            _ => {
                // Other segment types (for now, just skip the data)
                reader.skip(segment_data_length as u64 * 8)?; // Skip bits
            }
        }

        // Check if we should stop
        if segment_type == 51 {
            // Type 51 is end of page
            break;
        }
    }

    // Placeholder implementation: create a small black image
    let width = 64;
    let height = 64;
    let img: GrayImage = ImageBuffer::new(width, height);

    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_decode_no_header() {
        // A minimal JBIG2 file with a valid header (you will need to create this)
        let data =
            fs::read("test_data/no_header.jbig2").expect("Unable to read valid_header.jbig2");
        let result = decode_jbig2(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_symbol_dictionary() {
        // JBIG2 data with a Symbol Dictionary segment.
        let data = fs::read("test_data/symbol_dictionary.jbig2")
            .expect("Unable to read symbol_dictionary.jbig2");
        let result = decode_jbig2(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_image_region() {
        // JBIG2 data with an Image Region segment.
        let data =
            fs::read("test_data/image_region.jbig2").expect("Unable to read image_region.jbig2");
        let result = decode_jbig2(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_image_region_with_symbol_dictionary() {
        // JBIG2 data with an Image Region segment referencing a Symbol Dictionary segment.
        let data = fs::read("test_data/image_region_with_symbol_dictionary.jbig2")
            .expect("Unable to read image_region_with_symbol_dictionary.jbig2");
        decode_jbig2(&data).unwrap();
    }
}
