use crate::av::{Packet, CodecType};
use std::collections::HashMap;

pub const TS_PACKET_SIZE: usize = 188;
pub const PAT_PID: u16 = 0;
pub const PMT_PID: u16 = 0x100;
pub const PCR_PID: u16 = 0x100;

pub struct TsStream {
    pub codec_type: CodecType,
    pub pid: u16,
    pub stream_type: u8,
    pub continuity_counter: u8,
}

pub struct TsMuxer {
    pub streams: HashMap<i8, TsStream>,
    pub pat_cc: u8,
    pub pmt_cc: u8,
}

impl TsMuxer {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            pat_cc: 0,
            pmt_cc: 0,
        }
    }

    pub fn add_stream(&mut self, idx: i8, codec_type: CodecType) {
        let pid = 0x100 + (idx as u16) + 1;
        let stream_type = match codec_type {
            CodecType::H264 => 0x1b, // H264 Stream Type
            CodecType::H265 => 0x24, // H265 Stream Type
            CodecType::AAC  => 0x0f, // AAC ADTS Stream Type
            _ => 0,
        };
        self.streams.insert(idx, TsStream {
            codec_type,
            pid,
            stream_type,
            continuity_counter: 0,
        });
    }

    // Generate initial PAT and PMT TS packets (needed at start of TS files)
    pub fn get_pat_pmt(&mut self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        // 1. PAT Packet (188 bytes)
        let mut pat_pkt = vec![0u8; TS_PACKET_SIZE];
        pat_pkt[0] = 0x47; // Sync byte
        pat_pkt[1] = 0x40 | ((PAT_PID >> 8) as u8); // Payload start + PID high
        pat_pkt[2] = (PAT_PID & 0xff) as u8;       // PID low
        pat_pkt[3] = 0x10 | (self.pat_cc & 0xf);   // No adaptation, payload only + CC
        self.pat_cc = (self.pat_cc + 1) % 16;
        
        pat_pkt[4] = 0; // Pointer field

        // PAT Table payload
        let pat_payload = [
            0x00, // Table ID
            0xb0, 0x0d, // Section syntax, length 13
            0x00, 0x01, // Transport Stream ID
            0xc1, // Version 0, current next indicator
            0x00, 0x00, // Section number / last section
            0x00, 0x01, // Program number 1
            0xe0 | ((PMT_PID >> 8) as u8), (PMT_PID & 0xff) as u8, // PMT PID
            0x2a, 0xb1, 0x04, 0xb2, // Dummy CRC32
        ];
        pat_pkt[5..5+pat_payload.len()].copy_from_slice(&pat_payload);
        
        // Fill rest with 0xff padding
        for i in (5 + pat_payload.len())..TS_PACKET_SIZE {
            pat_pkt[i] = 0xff;
        }
        buf.extend_from_slice(&pat_pkt);

        // 2. PMT Packet (188 bytes)
        let mut pmt_pkt = vec![0u8; TS_PACKET_SIZE];
        pmt_pkt[0] = 0x47; // Sync byte
        pmt_pkt[1] = 0x40 | ((PMT_PID >> 8) as u8);
        pmt_pkt[2] = (PMT_PID & 0xff) as u8;
        pmt_pkt[3] = 0x10 | (self.pmt_cc & 0xf);
        self.pmt_cc = (self.pmt_cc + 1) % 16;

        pmt_pkt[4] = 0; // Pointer field

        // PMT payload (build dynamically depending on registered streams)
        let mut pmt_table = vec![
            0x02, // Table ID PMT
            0xb0, 0x00, // Section length (placeholder at index 1-2)
            0x00, 0x01, // Program number
            0xc1, // Version 0
            0x00, 0x00, // Section number
            0xe0 | ((PCR_PID >> 8) as u8), (PCR_PID & 0xff) as u8, // PCR PID
            0xf0, 0x00, // Program info length
        ];

        for stream in self.streams.values() {
            pmt_table.push(stream.stream_type); // Stream type
            pmt_table.push(0xe0 | ((stream.pid >> 8) as u8)); // PID high
            pmt_table.push((stream.pid & 0xff) as u8);        // PID low
            pmt_table.push(0xf0); // ES Info length high
            pmt_table.push(0x00); // ES Info length low
        }

        // Add dummy CRC32
        pmt_table.extend_from_slice(&[0x3f, 0xe4, 0x5a, 0x82]);

        // Fix PMT Section Length (total length minus first 3 bytes)
        let len = (pmt_table.len() - 3) as u16;
        pmt_table[1] = 0xb0 | ((len >> 8) as u8);
        pmt_table[2] = (len & 0xff) as u8;

        pmt_pkt[5..5+pmt_table.len()].copy_from_slice(&pmt_table);
        for i in (5 + pmt_table.len())..TS_PACKET_SIZE {
            pmt_pkt[i] = 0xff;
        }
        buf.extend_from_slice(&pmt_pkt);

        buf
    }

    // Packetize PES video/audio data into 188-byte TS packets
    pub fn write_packet(&mut self, pkt: &Packet) -> Vec<u8> {
        let stream = match self.streams.get_mut(&pkt.idx) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut buf = Vec::new();
        
        // 1. Build PES Payload
        let mut pes = Vec::new();
        
        // PES Start code prefix 0x000001
        pes.extend_from_slice(&[0x00, 0x00, 0x01]);
        
        // Stream ID
        let stream_id = match stream.codec_type {
            CodecType::H264 | CodecType::H265 => 0xe0, // Video stream 1
            CodecType::AAC => 0xc0,                  // Audio stream 1
            _ => 0xbd,
        };
        pes.push(stream_id);
        
        // PES Packet Length (0 for unlimited video, or size for audio)
        if stream_id >= 0xe0 {
            pes.extend_from_slice(&[0, 0]); // Unlimited video length
        } else {
            let pes_len = (pkt.data.len() + 8) as u16; // raw packet + PES header extension
            pes.extend_from_slice(&pes_len.to_be_bytes());
        }

        // PES Flags
        pes.extend_from_slice(&[0x84, 0xc0]); // PTS & DTS present, original or copy
        pes.push(10); // Header data length (5 bytes PTS + 5 bytes DTS)

        // Write PTS & DTS timestamps (converted to 90khz clock scale)
        let pts = (pkt.time.as_secs_f64() * 90000.0) as u64;
        let dts = pts - ((pkt.composition_time.as_secs_f64() * 90000.0) as u64);

        // Encode PTS
        pes.push((((pts >> 29) & 0x07) << 1 | 0x31) as u8);
        pes.extend_from_slice(&(((pts >> 14) & 0x7fff) << 1 | 1).to_be_bytes());
        pes.extend_from_slice(&(((pts & 0x7fff) << 1) | 1).to_be_bytes());

        // Encode DTS
        pes.push((((dts >> 29) & 0x07) << 1 | 0x11) as u8);
        pes.extend_from_slice(&(((dts >> 14) & 0x7fff) << 1 | 1).to_be_bytes());
        pes.extend_from_slice(&(((dts & 0x7fff) << 1) | 1).to_be_bytes());

        // For video codecs, prepend AUD (Access Unit Delimiter) and H264 start codes if keyframe
        if stream.codec_type == CodecType::H264 || stream.codec_type == CodecType::H265 {
            pes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x09, 0xf0]); // AUD NAL Unit
        }
        
        // Append raw packet data
        pes.extend_from_slice(&pkt.data);

        // 2. Slice PES payload into 188-byte TS packets
        let mut offset = 0;
        let mut is_first_packet = true;

        while offset < pes.len() {
            let mut ts_pkt = vec![0u8; TS_PACKET_SIZE];
            ts_pkt[0] = 0x47; // Sync byte
            
            let mut flags_pid = stream.pid;
            if is_first_packet {
                flags_pid |= 0x4000; // Payload unit start indicator
            }
            ts_pkt[1] = (flags_pid >> 8) as u8;
            ts_pkt[2] = (flags_pid & 0xff) as u8;

            let remaining_pes = pes.len() - offset;
            let cc = stream.continuity_counter;
            stream.continuity_counter = (cc + 1) % 16;

            if is_first_packet {
                // First packet needs adaptation field if PCR is sent or for keyframes
                let has_adaptation = pkt.is_key_frame;
                if has_adaptation {
                    ts_pkt[3] = 0x30 | (cc & 0xf); // Adaptation + payload
                    
                    // Simple adaptation header: length 7, random access flag, PCR flag
                    ts_pkt[4] = 7; // Adaptation field length
                    ts_pkt[5] = 0x50; // Random access indicator + PCR flag
                    
                    // Encode PCR (Program Clock Reference) matching DTS
                    let pcr_base = dts;
                    ts_pkt[6] = (pcr_base >> 25) as u8;
                    ts_pkt[7] = ((pcr_base >> 17) & 0xff) as u8;
                    ts_pkt[8] = ((pcr_base >> 9) & 0xff) as u8;
                    ts_pkt[9] = ((pcr_base >> 1) & 0xff) as u8;
                    ts_pkt[10] = (((pcr_base & 1) << 7) | 0x7e) as u8;
                    ts_pkt[11] = 0; // PCR extension

                    let payload_size = TS_PACKET_SIZE - 12;
                    let to_copy = remaining_pes.min(payload_size);
                    ts_pkt[12..12+to_copy].copy_from_slice(&pes[offset..offset+to_copy]);
                    
                    // Fill remaining space with 0xff padding
                    if to_copy < payload_size {
                        let fill_start = 12 + to_copy;
                        // Adjust adaptation field length to absorb padding
                        ts_pkt[4] += (payload_size - to_copy) as u8;
                        for i in fill_start..TS_PACKET_SIZE {
                            ts_pkt[i] = 0xff;
                        }
                    }
                    offset += to_copy;
                } else {
                    ts_pkt[3] = 0x10 | (cc & 0xf); // Payload only
                    let payload_size = TS_PACKET_SIZE - 4;
                    let to_copy = remaining_pes.min(payload_size);
                    ts_pkt[4..4+to_copy].copy_from_slice(&pes[offset..offset+to_copy]);
                    offset += to_copy;
                }
                is_first_packet = false;
            } else {
                // Secondary packets
                if remaining_pes >= (TS_PACKET_SIZE - 4) {
                    ts_pkt[3] = 0x10 | (cc & 0xf); // Payload only
                    ts_pkt[4..TS_PACKET_SIZE].copy_from_slice(&pes[offset..offset + (TS_PACKET_SIZE - 4)]);
                    offset += TS_PACKET_SIZE - 4;
                } else {
                    // Padding required for last packet
                    ts_pkt[3] = 0x30 | (cc & 0xf); // Adaptation + payload
                    let padding_len = TS_PACKET_SIZE - 4 - 1 - remaining_pes;
                    
                    ts_pkt[4] = (padding_len + 1) as u8; // Adaptation length (padding + 1 byte flags)
                    ts_pkt[5] = 0x00; // No flags
                    
                    // Fill adaptation padding with 0xff
                    let payload_start = 6 + padding_len;
                    for i in 6..payload_start {
                        ts_pkt[i] = 0xff;
                    }
                    
                    // Copy remaining payload at the end
                    ts_pkt[payload_start..TS_PACKET_SIZE].copy_from_slice(&pes[offset..]);
                    offset += remaining_pes;
                }
            }
            buf.extend_from_slice(&ts_pkt);
        }

        buf
    }
}
