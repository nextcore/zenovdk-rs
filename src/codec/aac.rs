use crate::utils::bits::GolombBitReader;

#[derive(Debug, Default, Clone, Copy)]
pub struct MPEG4AudioConfig {
    pub sample_rate: u32,
    pub object_type: u32,
    pub sample_rate_index: u32,
    pub channel_config: u32,
}

const SAMPLE_RATE_TABLE: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000,
    24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

impl MPEG4AudioConfig {
    pub fn complete(&mut self) {
        if (self.sample_rate_index as usize) < SAMPLE_RATE_TABLE.len() {
            self.sample_rate = SAMPLE_RATE_TABLE[self.sample_rate_index as usize];
        }
    }
}

pub fn parse_adts_header(frame: &[u8]) -> Result<(MPEG4AudioConfig, usize, usize, usize), &'static str> {
    if frame.len() < 7 {
        return Err("adts frame too short");
    }
    if frame[0] != 0xff || (frame[1] & 0xf6) != 0xf0 {
        return Err("not adts header");
    }

    let mut config = MPEG4AudioConfig::default();
    config.object_type = ((frame[2] >> 6) as u32) + 1;
    config.sample_rate_index = ((frame[2] >> 2) & 0xf) as u32;
    config.channel_config = (((frame[2] << 2) & 0x4) | ((frame[3] >> 6) & 0x3)) as u32;

    if config.channel_config == 0 {
        return Err("adts channel count invalid");
    }
    config.complete();

    let framelen = (((frame[3] & 0x3) as usize) << 11) | ((frame[4] as usize) << 3) | ((frame[5] >> 5) as usize);
    let samples = (((frame[6] & 0x3) as usize) + 1) * 1024;
    
    let mut hdrlen = 7;
    if (frame[1] & 0x1) == 0 {
        hdrlen = 9;
    }
    if framelen < hdrlen {
        return Err("adts framelen < hdrlen");
    }

    Ok((config, hdrlen, framelen, samples))
}

pub fn parse_mpeg4_audio_config_bytes(data: &[u8]) -> Result<MPEG4AudioConfig, &'static str> {
    let mut br = GolombBitReader::new(data);
    let mut config = MPEG4AudioConfig::default();

    let mut object_type = br.read_bits(5)?;
    if object_type == 31 {
        let i = br.read_bits(6)?;
        object_type = 32 + i;
    }
    config.object_type = object_type;

    let mut index = br.read_bits(4)?;
    if index == 0xf {
        index = br.read_bits(24)?;
    }
    config.sample_rate_index = index;
    config.channel_config = br.read_bits(4)?;
    config.complete();

    Ok(config)
}
