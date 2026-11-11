use crate::{Result, need};

/// OpenAI's buffered WAV can retain streaming 0xffffffff chunk lengths.
/// Finalize only this exact PCM16 header; the sample payload is unchanged.
pub fn normalize_openai_wav(mut bytes: Vec<u8>) -> Result<Vec<u8>> {
    if bytes.len() < 44
        || &bytes[0..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
        || &bytes[12..16] != b"fmt "
        || &bytes[36..40] != b"data"
        || bytes[4..8] != [255; 4]
        || bytes[40..44] != [255; 4]
    {
        return Ok(bytes);
    }
    let u16_at = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let channels = u16_at(22) as usize;
    let align = u16_at(32) as usize;
    let rate = u32_at(24) as usize;
    need(
        u32_at(16) == 16
            && u16_at(20) == 1
            && [1, 2].contains(&channels)
            && u16_at(34) == 16
            && align == channels * 2
            && (8000..=48000).contains(&rate)
            && u32_at(28) as usize == rate * align
            && bytes.len() > 44
            && (bytes.len() - 44) % align == 0
            && bytes.len() <= crate::models::MAX_MEDIA,
        "invalid_provider_media",
    )?;
    let riff = (bytes.len() - 8) as u32;
    let data = (bytes.len() - 44) as u32;
    bytes[4..8].copy_from_slice(&riff.to_le_bytes());
    bytes[40..44].copy_from_slice(&data.to_le_bytes());
    Ok(bytes)
}

pub fn pcm16_wav(bytes: Vec<u8>, rate: u64, channels: u64) -> Result<Vec<u8>> {
    need(
        (8000..=48000).contains(&rate)
            && [1, 2].contains(&channels)
            && !bytes.is_empty()
            && bytes.len() <= crate::models::MAX_MEDIA - 44
            && bytes.len() % (channels as usize * 2) == 0,
        "invalid_provider_media",
    )?;
    let mut header = vec![0_u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&((bytes.len() + 36) as u32).to_le_bytes());
    header[8..16].copy_from_slice(b"WAVEfmt ");
    header[16..20].copy_from_slice(&16_u32.to_le_bytes());
    header[20..22].copy_from_slice(&1_u16.to_le_bytes());
    header[22..24].copy_from_slice(&(channels as u16).to_le_bytes());
    header[24..28].copy_from_slice(&(rate as u32).to_le_bytes());
    header[28..32].copy_from_slice(&((rate * channels * 2) as u32).to_le_bytes());
    header[32..34].copy_from_slice(&((channels * 2) as u16).to_le_bytes());
    header[34..36].copy_from_slice(&16_u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    header.extend(bytes);
    Ok(header)
}
