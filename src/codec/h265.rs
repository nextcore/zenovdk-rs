use crate::utils::bits::GolombBitReader;

#[derive(Debug, Default, Clone)]
pub struct SPSInfo {
    pub profile_idc: u32,
    pub level_idc: u32,
    pub mb_width: u32,
    pub mb_height: u32,
    pub crop_left: u32,
    pub crop_right: u32,
    pub crop_top: u32,
    pub crop_bottom: u32,
    pub width: u32,
    pub height: u32,
    pub num_temporal_layers: u32,
    pub temporal_id_nested: u32,
    pub chroma_format: u32,
    pub pic_width_in_luma_samples: u32,
    pub pic_height_in_luma_samples: u32,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub general_profile_space: u32,
    pub general_tier_flag: u32,
    pub general_profile_idc: u32,
    pub general_profile_compatibility_flags: u32,
    pub general_constraint_indicator_flags: u64,
    pub general_level_idc: u32,
    pub fps: u32,
}

pub fn remove_emulation_bytes(nal: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(nal.len());
    let mut i = 0;
    while i < nal.len() {
        if i + 2 < nal.len() && nal[i] == 0 && nal[i+1] == 0 && nal[i+2] == 3 {
            r.push(0);
            r.push(0);
            i += 3;
        } else {
            r.push(nal[i]);
            i += 1;
        }
    }
    r
}

pub fn parse_sps(sps: &[u8]) -> Result<SPSInfo, &'static str> {
    if sps.len() < 2 {
        return Err("Incorrect Unit Size");
    }
    
    // Skip 2 bytes NAL header
    let rbsp = remove_emulation_bytes(&sps[2..]);
    let mut br = GolombBitReader::new(&rbsp);

    br.read_bits(4)?; // sps_video_parameter_set_id
    let sps_max_sub_layers_minus1 = br.read_bits(3)?;
    
    let mut ctx = SPSInfo::default();
    ctx.num_temporal_layers = sps_max_sub_layers_minus1 + 1;
    ctx.temporal_id_nested = br.read_bit()?;

    parse_ptl(&mut br, &mut ctx, sps_max_sub_layers_minus1)?;

    br.read_exponential_golomb()?; // sps_seq_parameter_set_id
    ctx.chroma_format = br.read_exponential_golomb()?;
    if ctx.chroma_format == 3 {
        br.read_bit()?; // separate_colour_plane_flag
    }

    ctx.pic_width_in_luma_samples = br.read_exponential_golomb()?;
    ctx.width = ctx.pic_width_in_luma_samples;

    ctx.pic_height_in_luma_samples = br.read_exponential_golomb()?;
    ctx.height = ctx.pic_height_in_luma_samples;

    let conformance_window_flag = br.read_bit()?;
    if conformance_window_flag != 0 {
        br.read_exponential_golomb()?; // conf_win_left_offset
        br.read_exponential_golomb()?; // conf_win_right_offset
        br.read_exponential_golomb()?; // conf_win_top_offset
        br.read_exponential_golomb()?; // conf_win_bottom_offset
    }

    ctx.bit_depth_luma_minus8 = br.read_exponential_golomb()?;
    ctx.bit_depth_chroma_minus8 = br.read_exponential_golomb()?;

    br.read_exponential_golomb()?; // log2_max_pic_order_cnt_lsb_minus4

    let sps_sub_layer_ordering_info_present_flag = br.read_bit()?;
    let start_idx = if sps_sub_layer_ordering_info_present_flag != 0 { 0 } else { sps_max_sub_layers_minus1 };
    
    for _ in start_idx..=sps_max_sub_layers_minus1 {
        br.read_exponential_golomb()?; // max_dec_pic_buffering_minus1
        br.read_exponential_golomb()?; // max_num_reorder_pics
        br.read_exponential_golomb()?; // max_latency_increase_plus1
    }

    br.read_exponential_golomb()?; // log2_min_luma_coding_block_size_minus3
    br.read_exponential_golomb()?; // log2_diff_max_min_luma_coding_block_size
    br.read_exponential_golomb()?; // log2_min_luma_transform_block_size_minus2
    br.read_exponential_golomb()?; // log2_diff_max_min_luma_transform_block_size
    br.read_exponential_golomb()?; // max_transform_hierarchy_depth_inter
    br.read_exponential_golomb()?; // max_transform_hierarchy_depth_intra

    Ok(ctx)
}

fn parse_ptl(br: &mut GolombBitReader, ctx: &mut SPSInfo, max_sub_layers_minus1: u32) -> Result<(), &'static str> {
    let general_profile_space = br.read_bits(2)?;
    let general_tier_flag = br.read_bit()?;
    let general_profile_idc = br.read_bits(5)?;
    let general_profile_compatibility_flags = br.read_bits(16)? << 16 | br.read_bits(16)?;
    
    let general_constraint_indicator_flags = 
        ((br.read_bits(16)? as u64) << 32) | 
        ((br.read_bits(16)? as u64) << 16) | 
        (br.read_bits(16)? as u64);
        
    let general_level_idc = br.read_bits(8)?;

    let mut ptl = SPSInfo::default();
    ptl.general_profile_space = general_profile_space;
    ptl.general_tier_flag = general_tier_flag;
    ptl.general_profile_idc = general_profile_idc;
    ptl.general_profile_compatibility_flags = general_profile_compatibility_flags;
    ptl.general_constraint_indicator_flags = general_constraint_indicator_flags;
    ptl.general_level_idc = general_level_idc;

    update_ptl(ctx, &ptl);

    if max_sub_layers_minus1 == 0 {
        return Ok(());
    }

    let mut sub_layer_profile_present_flag = vec![0; max_sub_layers_minus1 as usize];
    let mut sub_layer_level_present_flag = vec![0; max_sub_layers_minus1 as usize];
    for i in 0..(max_sub_layers_minus1 as usize) {
        sub_layer_profile_present_flag[i] = br.read_bit()?;
        sub_layer_level_present_flag[i] = br.read_bit()?;
    }

    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            br.read_bits(2)?; // reserved_zero_2bits
        }
    }

    for i in 0..(max_sub_layers_minus1 as usize) {
        if sub_layer_profile_present_flag[i] != 0 {
            br.read_bits(16)?; br.read_bits(16)?; // profile space/tier/compatibility
            br.read_bits(16)?; br.read_bits(16)?;
            br.read_bits(16)?; br.read_bits(8)?;
        }
        if sub_layer_level_present_flag[i] != 0 {
            br.read_bits(8)?; // sub_layer_level_idc
        }
    }

    Ok(())
}

fn update_ptl(ctx: &mut SPSInfo, ptl: &SPSInfo) {
    ctx.general_profile_space = ptl.general_profile_space;
    if ptl.general_tier_flag > ctx.general_tier_flag {
        ctx.general_level_idc = ptl.general_level_idc;
        ctx.general_tier_flag = ptl.general_tier_flag;
    } else if ptl.general_level_idc > ctx.general_level_idc {
        ctx.general_level_idc = ptl.general_level_idc;
    }
    if ptl.general_profile_idc > ctx.general_profile_idc {
        ctx.general_profile_idc = ptl.general_profile_idc;
    }
    ctx.general_profile_compatibility_flags &= ptl.general_profile_compatibility_flags;
    ctx.general_constraint_indicator_flags &= ptl.general_constraint_indicator_flags;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceType {
    P = 1,
    B = 2,
    I = 3,
}

pub fn parse_slice_header_from_nalu(packet: &[u8]) -> Result<SliceType, &'static str> {
    if packet.len() <= 2 {
        return Err("packet too short to parse slice header");
    }
    
    // H265 NAL unit type is in bits 1-6 of the first 2 bytes header
    let nal_unit_type = (packet[0] >> 1) & 0x3f;
    match nal_unit_type {
        0..=9 | 16..=21 => {} // trailing, TSA, STSA, BLA, IDR, CRA slices
        _ => return Err("nal_unit_type has no slice header"),
    }

    let mut r = GolombBitReader::new(&packet[2..]);
    r.read_exponential_golomb()?; // slice_pic_parameter_set_id
    
    // Assume slice_type parsing logic matching slice_layer_rbsp syntax
    let slice_type = r.read_exponential_golomb()?;
    match slice_type {
        0 | 3 | 5 | 8 => Ok(SliceType::P),
        1 | 6 => Ok(SliceType::B),
        2 | 4 | 7 | 9 => Ok(SliceType::I),
        _ => Err("slice_type invalid"),
    }
}
