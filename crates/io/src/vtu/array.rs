//! Numeric DataArrays, including the byte-count and block-compression headers defined by VTK.
//! Appended offsets address the original encoded bytes after the underscore.

use std::borrow::Cow;

use base64::Engine;
use flate2::{Decompress, FlushDecompress, Status};
use roxmltree::Node;

use super::{attr, bad, components, one};
use crate::external::MAX_FRAME_BYTES;
use crate::{Error, Result};

const MAX_BLOCKS: usize = 65_536;

#[derive(Clone, Copy)]
struct Header {
    little: bool,
    width: usize,
    zlib: bool,
}

impl Header {
    fn word(&self, bytes: &mut &[u8]) -> Result<usize> {
        let (word, rest) = bytes.split_at_checked(self.width).ok_or_else(|| bad("truncated binary array header"))?;
        *bytes = rest;
        let value = match (self.width, self.little) {
            (4, true) => u64::from(u32::from_le_bytes(word.try_into().map_err(bad)?)),
            (4, false) => u64::from(u32::from_be_bytes(word.try_into().map_err(bad)?)),
            (_, true) => u64::from_le_bytes(word.try_into().map_err(bad)?),
            (_, false) => u64::from_be_bytes(word.try_into().map_err(bad)?),
        };
        usize::try_from(value).map_err(|_| bad("binary array size exceeds this platform"))
    }

    fn unpack<'a>(&self, mut encoded: &'a [u8], expected: usize, inline: bool) -> Result<Cow<'a, [u8]>> {
        if !self.zlib {
            if self.word(&mut encoded)? != expected {
                return Err(bad("binary byte count does not match the array dimensions"));
            }
            let (data, rest) = encoded.split_at_checked(expected).ok_or_else(|| bad("truncated binary array"))?;
            if inline && !rest.is_empty() {
                return Err(bad("excess bytes in binary array"));
            }
            return Ok(Cow::Borrowed(data));
        }

        let blocks = self.word(&mut encoded)?;
        let block_size = self.word(&mut encoded)?;
        let last_size = self.word(&mut encoded)?;
        // A default VTK compressor block is 32 KiB. Cap even very small blocks to
        // bound the number of new decompressor states a single array can request.
        if blocks > MAX_BLOCKS {
            return Err(Error::Limit("too many zlib blocks in a VTU array".into()));
        }
        if last_size > block_size || (blocks > 0 && block_size == 0) || (blocks == 0 && (expected != 0 || last_size != 0)) {
            return Err(bad("invalid zlib block sizes"));
        }
        let last_size = if blocks > 0 && last_size == 0 { block_size } else { last_size };
        let total = blocks.saturating_sub(1).checked_mul(block_size).and_then(|n| n.checked_add(last_size));
        if total != Some(expected) {
            return Err(bad("zlib byte count does not match the array dimensions"));
        }
        let table_bytes = blocks.checked_mul(self.width).ok_or_else(|| bad("invalid zlib block count"))?;
        let (mut table, mut compressed) = encoded.split_at_checked(table_bytes).ok_or_else(|| bad("truncated zlib block table"))?;
        // One spare byte distinguishes a block expanding beyond its declared length.
        let mut output = vec![0_u8; expected + 1];
        let mut start = 0;
        for i in 0..blocks {
            let size = self.word(&mut table)?;
            let (block, rest) = compressed.split_at_checked(size).ok_or_else(|| bad("truncated zlib block"))?;
            compressed = rest;
            let limit = if i + 1 == blocks { last_size } else { block_size };
            let buffer = output.get_mut(start..=start + limit).ok_or_else(|| bad("invalid zlib output length"))?;
            let mut decoder = Decompress::new(true);
            let status = decoder.decompress(block, buffer, FlushDecompress::Finish).map_err(bad)?;
            if status != Status::StreamEnd || decoder.total_in() != size as u64 || decoder.total_out() != limit as u64 {
                return Err(bad("incomplete or oversized zlib block"));
            }
            start += limit;
        }
        if inline && !compressed.is_empty() {
            return Err(bad("excess bytes after zlib blocks"));
        }
        output.truncate(expected);
        Ok(Cow::Owned(output))
    }
}

struct Appended<'i> {
    data: &'i [u8],
    base64: bool,
    offsets: Vec<usize>,
}

pub(super) struct Decoder<'i> {
    header: Header,
    appended: Option<Appended<'i>>,
    remaining_values: usize,
}

impl<'i> Decoder<'i> {
    pub(super) fn new(root: Node<'_, '_>, payload: Option<&'i [u8]>) -> Result<Self> {
        let little = match root.attribute("byte_order").unwrap_or("LittleEndian") {
            "LittleEndian" => true,
            "BigEndian" => false,
            order => return Err(bad(format!("unsupported byte_order {order}"))),
        };
        let width = match root.attribute("header_type").unwrap_or("UInt32") {
            "UInt32" => 4,
            "UInt64" => 8,
            kind => return Err(bad(format!("unsupported header_type {kind}"))),
        };
        let zlib = match root.attribute("compressor") {
            None | Some("") => false,
            Some("vtkZLibDataCompressor") => true,
            Some(name) => return Err(bad(format!("unsupported compressor {name}"))),
        };
        let appended = if let Some(data) = payload {
            let tag = one(root, "AppendedData")?;
            let base64 = match tag.attribute("encoding").unwrap_or("base64") {
                "raw" => false,
                "base64" => true,
                other => return Err(bad(format!("unsupported AppendedData encoding {other}"))),
            };
            let mut offsets = Vec::new();
            for node in root.descendants().filter(|n| n.has_tag_name("DataArray") && n.attribute("format") == Some("appended")) {
                let offset = attr(node, "offset")?.parse::<usize>().map_err(bad)?;
                if offset >= data.len() {
                    return Err(bad("appended offset outside the payload"));
                }
                offsets.push(offset);
            }
            offsets.push(data.len());
            offsets.sort_unstable();
            if offsets.windows(2).any(|pair| pair.first() == pair.last()) {
                return Err(bad("appended DataArrays must use separate byte ranges"));
            }
            Some(Appended { data, base64, offsets })
        } else {
            None
        };
        Ok(Self { header: Header { little, width, zlib }, appended, remaining_values: MAX_FRAME_BYTES / size_of::<f64>() })
    }

    fn binary<'a>(&self, node: Node<'_, '_>, text: &'a [u8], format: &str) -> Result<(Cow<'a, [u8]>, bool)>
    where
        'i: 'a,
    {
        if format == "binary" {
            return Ok((Cow::Owned(decode64(text)?), true));
        }
        let appended = self.appended.as_ref().ok_or_else(|| bad("missing AppendedData"))?;
        let offset = attr(node, "offset")?.parse::<usize>().map_err(bad)?;
        let index = appended.offsets.binary_search(&offset).map_err(|_| bad("unknown array offset"))?;
        let end = appended.offsets.get(index + 1).ok_or_else(|| bad("offset outside the payload"))?;
        let span = appended.data.get(offset..*end).ok_or_else(|| bad("invalid appended data span"))?;
        if appended.base64 { Ok((Cow::Owned(decode64(span)?), true)) } else { Ok((Cow::Borrowed(span), false)) }
    }

    pub(super) fn values(&mut self, node: Node<'_, '_>, count: usize, integers_only: bool) -> Result<Vec<f64>> {
        if let Some(tuples) = node.attribute("NumberOfTuples") {
            let tuples = tuples.parse::<usize>().map_err(bad)?;
            if tuples.checked_mul(components(node)?) != Some(count) {
                return Err(bad("NumberOfTuples does not match the array dimensions"));
            }
        }
        self.remaining_values =
            self.remaining_values.checked_sub(count).ok_or_else(|| Error::Limit("VTU arrays exceed the import memory limit".into()))?;
        let kind = attr(node, "type")?;
        if integers_only && matches!(kind, "Float32" | "Float64") {
            return Err(bad("cell topology requires integer arrays"));
        }
        let format = node.attribute("format").unwrap_or("ascii");
        // XML comments and CDATA boundaries may split a numeric token or a base64 quartet.
        // Text in optional child elements (such as VTK InformationKey) is metadata.
        let mut parts = node.children().filter(|n| n.is_text()).filter_map(|n| n.text());
        let mut text = Cow::Borrowed(parts.next().unwrap_or(""));
        for part in parts {
            text.to_mut().push_str(part);
        }
        let binary = match format {
            "ascii" => None,
            "binary" | "appended" => Some(self.binary(node, text.as_bytes(), format)?),
            other => return Err(bad(format!("unsupported DataArray format {other}"))),
        };
        // Parse the declared primitive before converting to our common result-field storage.
        macro_rules! numbers {
            ($t:ty) => {{
                let mut values = Vec::new();
                if let Some((encoded, inline)) = binary {
                    let expected = count.checked_mul(size_of::<$t>()).ok_or_else(|| bad("array too large"))?;
                    let raw = self.header.unpack(&encoded, expected, inline)?;
                    values.reserve_exact(count);
                    for chunk in raw.as_chunks::<{ size_of::<$t>() }>().0 {
                        let value = if self.header.little { <$t>::from_le_bytes(*chunk) } else { <$t>::from_be_bytes(*chunk) };
                        values.push(value.into_field()?);
                    }
                } else {
                    for word in text.split_ascii_whitespace() {
                        if values.len() >= count {
                            return Err(bad("too many values in ASCII array"));
                        }
                        values.push(word.parse::<$t>().map_err(bad)?.into_field()?);
                    }
                    if values.len() != count {
                        return Err(bad("too few values in ASCII array"));
                    }
                }
                Ok(values)
            }};
        }
        match kind {
            "Float32" => numbers!(f32),
            "Float64" => numbers!(f64),
            "Int8" => numbers!(i8),
            "UInt8" => numbers!(u8),
            "Int16" => numbers!(i16),
            "UInt16" => numbers!(u16),
            "Int32" => numbers!(i32),
            "UInt32" => numbers!(u32),
            "Int64" => numbers!(i64),
            "UInt64" => numbers!(u64),
            other => Err(bad(format!("unsupported DataArray type {other}"))),
        }
    }

    pub(super) fn indices(&mut self, node: Node<'_, '_>, count: usize) -> Result<Vec<u32>> {
        self.values(node, count, true)?
            .into_iter()
            .map(|x| if (0.0..=f64::from(u32::MAX)).contains(&x) { Ok(x as u32) } else { Err(bad("negative or oversized cell topology index")) })
            .collect()
    }
}

trait IntoField {
    fn into_field(self) -> Result<f64>;
}

macro_rules! lossless {
    ($($t:ty),+) => {
        $(impl IntoField for $t {
            fn into_field(self) -> Result<f64> { Ok(f64::from(self)) }
        })+
    }
}
lossless!(f64, f32, i8, u8, i16, u16, i32, u32);

macro_rules! wide {
    ($($t:ty),+) => {
        $(impl IntoField for $t {
            fn into_field(self) -> Result<f64> {
                let value = self as f64;
                if value as i128 != i128::from(self) {
                    return Err(bad("64-bit field integer cannot be represented exactly"));
                }
                Ok(value)
            }
        })+
    }
}
wide!(i64, u64);

/// Writers may encode the header and data in one operation or as individually padded segments.
fn decode64(input: &[u8]) -> Result<Vec<u8>> {
    let data: Cow<'_, [u8]> = if input.iter().any(u8::is_ascii_whitespace) {
        Cow::Owned(input.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect())
    } else {
        Cow::Borrowed(input)
    };
    let mut out = Vec::new();
    let mut rest = data.as_ref();
    let mut segments = 0_usize;
    while !rest.is_empty() {
        if segments > MAX_BLOCKS {
            return Err(Error::Limit("too many base64 segments in a VTU array".into()));
        }
        segments += 1;
        let end = match rest.iter().position(|&b| b == b'=') {
            Some(pad) => pad + 1 + usize::from(rest.get(pad + 1) == Some(&b'=')),
            None => rest.len(),
        };
        let (segment, tail) = rest.split_at_checked(end).ok_or_else(|| bad("invalid base64 padding"))?;
        base64::engine::general_purpose::STANDARD.decode_vec(segment, &mut out).map_err(bad)?;
        rest = tail;
    }
    Ok(out)
}
