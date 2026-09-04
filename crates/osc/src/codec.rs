//! OSC 1.0 encode/decode matching MixLink `OSCCodec`.
//!
//! Address and type-tag strings are null-terminated and padded to 4 bytes.
//! Type tags: `,` then `f` / `i` / `s` / `T` / `F`. Numeric payloads are big-endian.
//! `#bundle` messages start at offset 16 (8-byte header + 8-byte timetag).

/// OSC argument.
#[derive(Clone, Debug, PartialEq)]
pub enum OscValue {
    Float(f32),
    Int(i32),
    String(String),
    Bool(bool),
}

impl OscValue {
    /// Numeric view used by inbound TotalMix handlers.
    pub fn float_value(&self) -> f32 {
        match self {
            Self::Float(f) => *f,
            Self::Int(i) => *i as f32,
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Self::String(s) => s.parse().unwrap_or(0.0),
        }
    }
}

/// One OSC address plus arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct OscMessage {
    pub address: String,
    pub values: Vec<OscValue>,
}

impl OscMessage {
    pub fn new(address: impl Into<String>, values: Vec<OscValue>) -> Self {
        Self { address: address.into(), values }
    }
}

/// Encode a single OSC message.
pub fn encode(message: &OscMessage) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&osc_string(&message.address));
    let mut tags = String::from(",");
    for v in &message.values {
        match v {
            OscValue::Float(_) => tags.push('f'),
            OscValue::Int(_) => tags.push('i'),
            OscValue::String(_) => tags.push('s'),
            OscValue::Bool(true) => tags.push('T'),
            OscValue::Bool(false) => tags.push('F'),
        }
    }
    data.extend_from_slice(&osc_string(&tags));
    for v in &message.values {
        match v {
            OscValue::Float(f) => data.extend_from_slice(&f.to_bits().to_be_bytes()),
            OscValue::Int(i) => data.extend_from_slice(&i.to_be_bytes()),
            OscValue::String(s) => data.extend_from_slice(&osc_string(s)),
            OscValue::Bool(_) => {}
        }
    }
    data
}

/// Decode a datagram. A `#bundle` expands to its contained messages.
pub fn decode(data: &[u8]) -> Vec<OscMessage> {
    if data.len() < 8 {
        return Vec::new();
    }
    if data.starts_with(b"#bundle") {
        return decode_bundle(data);
    }
    decode_message(data).into_iter().collect()
}

fn decode_bundle(data: &[u8]) -> Vec<OscMessage> {
    let mut messages = Vec::new();
    // "#bundle\0" (8) + timetag (8) → first element size at offset 16.
    let mut offset = 16usize;
    while offset + 4 <= data.len() {
        let size = int32(data, offset);
        offset += 4;
        let end = offset.saturating_add(size as usize);
        if size <= 0 || end > data.len() {
            break;
        }
        messages.extend(decode(&data[offset..end]));
        offset = end;
    }
    messages
}

fn decode_message(data: &[u8]) -> Option<OscMessage> {
    let mut offset = 0usize;
    let address = read_string(data, &mut offset)?;
    if !address.starts_with('/') {
        return None;
    }
    let Some(tags) = read_string(data, &mut offset) else {
        return Some(OscMessage { address, values: Vec::new() });
    };
    if !tags.starts_with(',') {
        return Some(OscMessage { address, values: Vec::new() });
    }
    let mut values = Vec::new();
    for tag in tags.chars().skip(1) {
        match tag {
            'f' => {
                values.push(OscValue::Float(float32(data, offset)));
                offset += 4;
            }
            'i' => {
                values.push(OscValue::Int(int32(data, offset)));
                offset += 4;
            }
            's' => {
                if let Some(s) = read_string(data, &mut offset) {
                    values.push(OscValue::String(s));
                }
            }
            'T' => values.push(OscValue::Bool(true)),
            'F' => values.push(OscValue::Bool(false)),
            _ => {}
        }
        if offset > data.len() {
            break;
        }
    }
    Some(OscMessage { address, values })
}

fn osc_string(s: &str) -> Vec<u8> {
    let mut d = s.as_bytes().to_vec();
    d.push(0);
    while d.len() % 4 != 0 {
        d.push(0);
    }
    d
}

fn read_string(data: &[u8], offset: &mut usize) -> Option<String> {
    if *offset >= data.len() {
        return None;
    }
    let mut end = *offset;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    let s = String::from_utf8_lossy(&data[*offset..end]).into_owned();
    end += 1;
    while end % 4 != 0 {
        end += 1;
    }
    *offset = end;
    Some(s)
}

fn int32(data: &[u8], offset: usize) -> i32 {
    if offset + 4 > data.len() {
        return 0;
    }
    i32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn float32(data: &[u8], offset: usize) -> f32 {
    f32::from_bits(int32(data, offset) as u32)
}

/// Build a `#bundle` datagram (tests / helpers). Timetag is zero (immediate).
pub fn encode_bundle(messages: &[OscMessage]) -> Vec<u8> {
    let mut data = Vec::from(*b"#bundle\0");
    data.extend_from_slice(&[0u8; 8]);
    for message in messages {
        let enc = encode(message);
        data.extend_from_slice(&(enc.len() as i32).to_be_bytes());
        data.extend_from_slice(&enc);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_float() {
        let msg = OscMessage::new("/mix/in/0/0/faderlin", vec![OscValue::Float(0.75)]);
        let decoded = decode(&encode(&msg));
        assert_eq!(decoded, vec![msg]);
    }

    #[test]
    fn encode_decode_int() {
        let msg = OscMessage::new("/test/int", vec![OscValue::Int(-7)]);
        let decoded = decode(&encode(&msg));
        assert_eq!(decoded, vec![msg]);
    }

    #[test]
    fn encode_decode_string() {
        let msg = OscMessage::new("/input/0/name", vec![OscValue::String("ADAT 1/2".into())]);
        let decoded = decode(&encode(&msg));
        assert_eq!(decoded, vec![msg]);
    }

    #[test]
    fn encode_decode_bool() {
        let t = OscMessage::new("/input/0/mute", vec![OscValue::Bool(true)]);
        let f = OscMessage::new("/input/0/solo", vec![OscValue::Bool(false)]);
        assert_eq!(decode(&encode(&t)), vec![t]);
        assert_eq!(decode(&encode(&f)), vec![f]);
    }

    #[test]
    fn encode_decode_bundle() {
        let a = OscMessage::new("/sendall", vec![OscValue::Float(1.0)]);
        let b = OscMessage::new("/input/1/name", vec![OscValue::String("Kick".into())]);
        let bytes = encode_bundle(&[a.clone(), b.clone()]);
        assert!(bytes.starts_with(b"#bundle"));
        assert_eq!(decode(&bytes), vec![a, b]);
    }
}
