use crate::{
    ObjectValueError, Result,
    graphics::NameOrDictByRef,
    object::{PdfObject as _, PdfObjectCore as _},
    text::{EncodingDict, EncodingDifferences, FontDescriptorFlags, FontDict},
};
use log::{error, info, warn};
use nipdf_cff_parser::{File as CffFile, Font as CffFont};
use prescript::{Encoding, Name, name, sname};
use snafu::{OptionExt as _, ResultExt};

pub type EncodingPair<'a> = (Option<Name>, Option<EncodingDifferences<'a>>);

/// Parser for font encodings
pub struct EncodingParser<'a, 'b, 'c>(pub &'c FontDict<'a, 'b>);

impl EncodingParser<'_, '_, '_> {
    fn by_name(name: &Name) -> Option<Encoding> {
        let r = Encoding::predefined(name);
        if r.is_none() {
            warn!("Unknown encoding: {}", name.as_str());
        }
        r
    }

    fn by_font_name(font_name: &Name) -> Option<Encoding> {
        let encoding_name = standard_14_type1_font_encoding(font_name);
        encoding_name.and_then(|n| Self::by_name(&n))
    }

    fn resolve_by_encoding_or_font_name(
        pair: &Option<EncodingPair<'_>>,
        font_name: &str,
    ) -> Option<Encoding> {
        pair.as_ref()
            .and_then(|p| p.0.as_ref().and_then(Self::by_name))
            .or_else(|| Self::by_font_name(&name(font_name)))
    }

    fn load_from_file(font_name: &str, font_data: &[u8], is_cff: bool) -> Result<Option<Encoding>> {
        if is_cff {
            info!("scan encoding from cff font. ({})", font_name);
            let cff_file: CffFile<'_> = CffFile::open(font_data)
                .whatever_context::<_, ObjectValueError>("Open cff file")?;
            let font: CffFont<'_> = cff_file
                .iter()
                .whatever_context::<_, ObjectValueError>("iter fonts from cff file")?
                .next()
                .whatever_context::<_, ObjectValueError>("no font in cff?")?;
            Ok(Some(
                font.encodings()
                    .whatever_context::<_, ObjectValueError>("parse cff file encodings")?,
            ))
        } else {
            info!("scan encoding from type1 font. ({})", font_name);
            let type1_font = prescript::Font::parse(font_data)
                .whatever_context::<_, ObjectValueError>("parse type1 font encoding")?;
            Ok(type1_font.take_encoding())
        }
    }

    fn guess_by_font_name(font_name: &str) -> Option<Encoding> {
        // if font not embed encoding, use known encoding for the two standard symbol fonts
        if let "Symbol" | "ZapfDingbats" = font_name {
            Some(Encoding::SYMBOL)
        } else {
            None
        }
    }

    fn default_encoding(&self) -> Result<Encoding> {
        if let Some(desc) = self.0.font_descriptor()? {
            if desc.flags()?.contains(FontDescriptorFlags::SYMBOLIC) {
                // If the font is symbolic, try to use the encoding from the FontDict
                if let Some(encoding_pair) = self.encoding_pair()? {
                    if let Some(encoding_name) = encoding_pair.0 {
                        if let Some(encoding) = Self::by_name(&encoding_name) {
                            return Ok(encoding);
                        }
                    }
                }
                warn!(
                    "Symbolic font '{}' no encoding in font dict and file, use empty encoding",
                    desc.font_name()?
                );
                return Ok(Encoding::default());
            }
        }

        Ok(Encoding::STANDARD)
    }

    fn apply_encoding_diff(encoding: Encoding, pair: &Option<EncodingPair<'_>>) -> Encoding {
        if let Some((_, Some(diff))) = pair {
            return diff.apply_differences(encoding);
        }
        encoding
    }

    pub fn type1(&self, is_cff: bool, font_data: &[u8]) -> Result<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let font_name = self
            .0
            .font_name()
            .whatever_context::<_, ObjectValueError>("get type1 font name")?;
        let r = Self::resolve_by_encoding_or_font_name(&encoding_pair, font_name.as_ref())
            .or_else(
                || match Self::load_from_file(font_name.as_ref(), font_data, is_cff) {
                    Ok(encoding) => encoding,
                    Err(e) => {
                        error!("Failed to load encoding from file: {}", e);
                        None
                    }
                },
            )
            .or_else(|| Self::guess_by_font_name(font_name.as_ref()))
            .map_or_else(|| self.default_encoding(), Ok)?;
        Ok(Self::apply_encoding_diff(r, &encoding_pair))
    }

    pub fn type3(&self) -> Result<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let r = Self::resolve_by_encoding_or_font_name(&encoding_pair, "")
            .map_or_else(|| self.default_encoding(), Ok)?;
        Ok(Self::apply_encoding_diff(r, &encoding_pair))
    }

    fn encoding_pair(&self) -> Result<Option<EncodingPair<'_>>> {
        let encoding = self.0.encoding()?;
        let Some(encoding) = encoding else {
            return Ok(None);
        };

        Ok(Some(match encoding {
            NameOrDictByRef::Name(name) => (Some(name.clone()), None),
            NameOrDictByRef::Dict(d) => {
                let encoding_dict = EncodingDict::new(d, self.0.resolver())
                    .whatever_context::<_, ObjectValueError>("create EncodingDict")?;
                let encoding_name = encoding_dict.base_encoding()?;
                (encoding_name, encoding_dict.differences()?)
            }
        }))
    }

    pub fn ttf(&self) -> Result<Option<Encoding>> {
        let pair = self.encoding_pair()?;
        let Some(pair) = pair else {
            return Ok(None);
        };

        let r = pair.0.as_ref().map_or_else(Encoding::default, |n| {
            Self::by_name(&n.clone()).unwrap_or_else(Encoding::default)
        });
        Ok(Some(Self::apply_encoding_diff(r, &Some(pair))))
    }
}

/// If font_name is a standard 14 font, return its Encoding name
fn standard_14_type1_font_encoding(font_name: &str) -> Option<Name> {
    match normalize_font_name(font_name) {
        "Courier"
        | "Courier-Bold"
        | "Courier-BoldOblique"
        | "Courier-Oblique"
        | "Helvetica"
        | "Helvetica-Bold"
        | "Helvetica-BoldOblique"
        | "Helvetica-Oblique"
        | "Times-Bold"
        | "Times-BoldItalic"
        | "Times-Italic"
        | "Times-Roman" => Some(sname("StandardEncoding")),
        "Symbol" => Some(sname("Symbol")),
        "ZapfDingbats" => Some(sname("ZapfDingbats")),
        _ => None,
    }
}

/// This function returns the standard 14 font if the font name is an known internal name.
fn normalize_font_name(name: &str) -> &str {
    match name {
        "Arial" | "ArialMT" | "Helvetica" => "Helvetica",
        "Arial,Bold" | "Arial-Bold" | "Arial-BoldMT" | "Helvetica,Bold" | "Helvetica-Bold" => {
            "Helvetica-Bold"
        }
        "Arial,BoldItalic"
        | "Arial-BoldItalic"
        | "Arial-BoldItalicMT"
        | "Helvetica,BoldItalic"
        | "Helvetica-BoldItalic"
        | "Helvetica-BoldOblique" => "Helvetica-BoldOblique",
        "Arial,Italic" | "Arial-Italic" | "Arial-ItalicMT" | "Helvetica,Italic"
        | "Helvetica-Italic" | "Helvetica-Oblique" => "Helvetica-Oblique",
        "Courier" | "CourierNew" | "CourierNewPSMT" => "Courier",
        "Courier,Bold"
        | "Courier-Bold"
        | "CourierNew,Bold"
        | "CourierNew-Bold"
        | "CourierNewPS-BoldMT" => "Courier-Bold",
        "Courier,BoldItalic"
        | "Courier-BoldOblique"
        | "CourierNew,BoldItalic"
        | "CourierNew-BoldItalic"
        | "CourierNewPS-BoldItalicMT" => "Courier-BoldOblique",
        "Courier,Italic"
        | "Courier-Oblique"
        | "CourierNew,Italic"
        | "CourierNew-Italic"
        | "CourierNewPS-ItalicMT" => "Courier-Oblique",
        "Symbol" | "Symbol,Bold" | "Symbol,BoldItalic" | "Symbol,Italic" => "Symbol",
        "Times-Bold"
        | "TimesNewRoman,Bold"
        | "TimesNewRoman-Bold"
        | "TimesNewRomanPS-Bold"
        | "TimesNewRomanPS-BoldMT"
        | "TimesNewRomanPSMT,Bold" => "Times-Bold",
        "Times-BoldItalic"
        | "TimesNewRoman,BoldItalic"
        | "TimesNewRoman-BoldItalic"
        | "TimesNewRomanPS-BoldItalic"
        | "TimesNewRomanPS-BoldItalicMT"
        | "TimesNewRomanPSMT,BoldItalic" => "Times-BoldItalic",
        "Times-Italic"
        | "TimesNewRoman,Italic"
        | "TimesNewRoman-Italic"
        | "TimesNewRomanPS-Italic"
        | "TimesNewRomanPS-ItalicMT"
        | "TimesNewRomanPSMT,Italic" => "Times-Italic",
        "Times-Roman" | "TimesNewRoman" | "TimesNewRomanPS" | "TimesNewRomanPSMT" => "Times-Roman",
        "ZapfDingbats" => "ZapfDingbats",
        others => others,
    }
}
