use crate::utils::bits::GolombBitReader;
use bytes::Bytes;
use std::time::Duration;

pub const NALU_SEI: u8 = 6;
pub const NALU_SPS: u8 = 7;
pub const NALU_PPS: u8 = 8;
pub const NALU_AUD: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NaluType {
    Raw = 0,
    Avcc = 1,
    AnnexB = 2,
}

pub fn remove_h264_or_h265_emulation_bytes(b: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if i + 2 < b.len() && b[i] == 0 && b[i+1] == 0 && b[i+2] == 3 {
            r.push(0);
            r.push(0);
            i += 3;
        } else {
            r.push(b[i]);
            i += 1;
        }
    }
    r
}

pub fn split_nalus(b: &[u8]) -> (Vec<Vec<u8>>, NaluType) {
    if b.len() < 4 {
        return (vec![b.to_vec()], NaluType::Raw);
    }

    let val3 = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
    let val4 = ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32);

    // Maybe AVCC (Starts with 4-byte big-endian size)
    if val4 <= b.len() as u32 {
        let mut nalus = Vec::new();
        let mut _val4 = val4 as usize;
        let mut _b = &b[4..];
        loop {
            if _val4 > _b.len() {
                break;
            }
            nalus.push(_b[.._val4].to_vec());
            _b = &_b[_val4..];
            if _b.len() < 4 {
                break;
            }
            _val4 = ((_b[0] as usize) << 24) | ((_b[1] as usize) << 16) | ((_b[2] as usize) << 8) | (_b[3] as usize);
            _b = &_b[4..];
        }
        if _b.is_empty() {
            return (nalus, NaluType::Avcc);
        }
    }

    // Annex B (Starts with 000001 or 00000001)
    if val3 == 1 || val4 == 1 {
        let mut nalus = Vec::new();
        let mut start = 0;
        let mut pos = 0;
        let mut _val3 = val3;
        let mut _val4 = val4;

        loop {
            if start != pos {
                nalus.push(b[start..pos].to_vec());
            }
            if _val3 == 1 {
                pos += 3;
            } else if _val4 == 1 {
                pos += 4;
            }
            start = pos;
            if start == b.len() {
                break;
            }
            _val3 = 0;
            _val4 = 0;

            while pos < b.len() {
                if pos + 2 < b.len() && b[pos] == 0 {
                    let v3 = ((b[pos] as u32) << 16) | ((b[pos+1] as u32) << 8) | (b[pos+2] as u32);
                    if v3 == 0 {
                        if pos + 3 < b.len() {
                            let v4 = b[pos+3] as u32;
                            if v4 == 1 {
                                _val4 = 1;
                                _val3 = 0;
                                break;
                            }
                        }
                    } else if v3 == 1 {
                        _val3 = 1;
                        _val4 = 0;
                        break;
                    }
                    pos += 1;
                } else {
                    pos += 1;
                }
            }
        }
        return (nalus, NaluType::AnnexB);
    }

    (vec![b.to_vec()], NaluType::Raw)
}

#[derive(Debug, Default, Clone)]
pub struct SPSInfo {
    pub id: u32,
    pub profile_idc: u32,
    pub level_idc: u32,
    pub constraint_set_flag: u32,
    pub mb_width: u32,
    pub mb_height: u32,
    pub crop_left: u32,
    pub crop_right: u32,
    pub crop_top: u32,
    pub crop_bottom: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

pub fn parse_sps(data: &[u8]) -> Result<SPSInfo, &'static str> {
    let raw_data = remove_h264_or_h265_emulation_bytes(data);
    let mut r = GolombBitReader::new(&raw_data);

    // Skip NALU header (1 byte)
    r.read_bits(8)?;

    let mut s = SPSInfo::default();
    s.profile_idc = r.read_bits(8)?;
    s.constraint_set_flag = r.read_bits(8)? >> 2;
    s.level_idc = r.read_bits(8)?;
    s.id = r.read_exponential_golomb()?;

    if s.profile_idc == 100 || s.profile_idc == 110 ||
       s.profile_idc == 122 || s.profile_idc == 244 ||
       s.profile_idc == 44  || s.profile_idc == 83  ||
       s.profile_idc == 86  || s.profile_idc == 118 {
        
        let chroma_format_idc = r.read_exponential_golomb()?;
        if chroma_format_idc == 3 {
            r.read_bit()?; // residual_colour_transform_flag
        }
        r.read_exponential_golomb()?; // bit_depth_luma_minus8
        r.read_exponential_golomb()?; // bit_depth_chroma_minus8
        r.read_bit()?; // qpprime_y_zero_transform_bypass_flag
        
        let seq_scaling_matrix_present_flag = r.read_bit()?;
        if seq_scaling_matrix_present_flag != 0 {
            for i in 0..8 {
                let seq_scaling_list_present_flag = r.read_bit()?;
                if seq_scaling_list_present_flag != 0 {
                    let size_of_scaling_list = if i < 6 { 16 } else { 64 };
                    let mut last_scale = 8i32;
                    let mut next_scale = 8i32;
                    for _ in 0..size_of_scaling_list {
                        if next_scale != 0 {
                            let delta_scale = r.read_se()?;
                            next_scale = (last_scale + delta_scale + 256) % 256;
                        }
                        if next_scale != 0 {
                            last_scale = next_scale;
                        }
                    }
                }
            }
        }
    }

    r.read_exponential_golomb()?; // log2_max_frame_num_minus4
    let pic_order_cnt_type = r.read_exponential_golomb()?;
    if pic_order_cnt_type == 0 {
        r.read_exponential_golomb()?; // log2_max_pic_order_cnt_lsb_minus4
    } else if pic_order_cnt_type == 1 {
        r.read_bit()?; // delta_pic_order_always_zero_flag
        r.read_se()?; // offset_for_non_ref_pic
        r.read_se()?; // offset_for_top_to_bottom_field
        let num_ref_frames_in_pic_order_cnt_cycle = r.read_exponential_golomb()?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            r.read_se()?;
        }
    }

    r.read_exponential_golomb()?; // max_num_ref_frames
    r.read_bit()?; // gaps_in_frame_num_value_allowed_flag

    s.mb_width = r.read_exponential_golomb()? + 1;
    s.mb_height = r.read_exponential_golomb()? + 1;

    let frame_mbs_only_flag = r.read_bit()?;
    if frame_mbs_only_flag == 0 {
        r.read_bit()?; // mb_adaptive_frame_field_flag
    }
    r.read_bit()?; // direct_8x8_inference_flag

    let frame_cropping_flag = r.read_bit()?;
    if frame_cropping_flag != 0 {
        s.crop_left = r.read_exponential_golomb()?;
        s.crop_right = r.read_exponential_golomb()?;
        s.crop_top = r.read_exponential_golomb()?;
        s.crop_bottom = r.read_exponential_golomb()?;
    }

    s.width = (s.mb_width * 16) - s.crop_left * 2 - s.crop_right * 2;
    s.height = ((2 - frame_mbs_only_flag) * s.mb_height * 16) - s.crop_top * 2 - s.crop_bottom * 2;

    let vui_parameter_present_flag = r.read_bit()?;
    if vui_parameter_present_flag != 0 {
        let aspect_ratio_info_present_flag = r.read_bit()?;
        if aspect_ratio_info_present_flag != 0 {
            let aspect_ratio_idc = r.read_bits(8)?;
            if aspect_ratio_idc == 255 {
                r.read_bits(16)?; // sar_width
                r.read_bits(16)?; // sar_height
            }
        }
        let overscan_info_present_flag = r.read_bit()?;
        if overscan_info_present_flag != 0 {
            r.read_bit()?; // overscan_appropriate_flag
        }
        let video_signal_type_present_flag = r.read_bit()?;
        if video_signal_type_present_flag != 0 {
            r.read_bits(3)?; // video_format
            r.read_bit()?; // video_full_range_flag
            let colour_description_present_flag = r.read_bit()?;
            if colour_description_present_flag != 0 {
                r.read_bits(8)?; // colour_primaries
                r.read_bits(8)?; // transfer_characteristics
                r.read_bits(8)?; // matrix_coefficients
            }
        }
        let chroma_loc_info_present_flag = r.read_bit()?;
        if chroma_loc_info_present_flag != 0 {
            r.read_exponential_golomb()?; // chroma_sample_loc_type_top_field
            r.read_exponential_golomb()?; // chroma_sample_loc_type_bottom_field
        }
        let timing_info_present_flag = r.read_bit()?;
        if timing_info_present_flag != 0 {
            let num_units_in_tick = r.read_bits(16)? << 16 | r.read_bits(16)?;
            let time_scale = r.read_bits(16)? << 16 | r.read_bits(16)?;
            if num_units_in_tick != 0 {
                s.fps = (time_scale as f64 / num_units_in_tick as f64 / 2.0).floor() as u32;
            }
        }
    }

    Ok(s)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceType {
    P = 1,
    B = 2,
    I = 3,
}

pub fn parse_slice_header_from_nalu(packet: &[u8]) -> Result<SliceType, &'static str> {
    if packet.len() <= 1 {
        return Err("packet too short to parse slice header");
    }

    let nal_unit_type = packet[0] & 0x1f;
    match nal_unit_type {
        1 | 2 | 5 | 19 => {} // layers containing slice data
        _ => return Err("nal_unit_type has no slice header"),
    }

    let mut r = GolombBitReader::new(&packet[1..]);
    r.read_exponential_golomb()?; // first_mb_in_slice
    let slice_type = r.read_exponential_golomb()?;

    match slice_type {
        0 | 3 | 5 | 8 => Ok(SliceType::P),
        1 | 6 => Ok(SliceType::B),
        2 | 4 | 7 | 9 => Ok(SliceType::I),
        _ => Err("slice_type invalid"),
    }
}
