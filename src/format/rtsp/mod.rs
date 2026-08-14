use crate::av::{Packet, CodecType};
use std::time::Duration;

pub struct RtspParser {
    buffer: Vec<u8>,
    video_channel: u8,
    audio_channel: u8,
    video_codec: CodecType,
    audio_codec: CodecType,
    video_idx: i8,
    audio_idx: i8,
    
    // Video assembly states
    fu_buffer: Vec<u8>,
    fu_started: bool,
    pre_video_ts: u64,
    
    // Timelines
    audio_timeline: Duration,
    audio_timescale: u32,
    
    // Outputs
    packet_queue: Vec<Packet>,
}

impl RtspParser {
    pub fn new(video_codec: CodecType, audio_codec: CodecType) -> Self {
        Self {
            buffer: Vec::new(),
            video_channel: 0,
            audio_channel: 2,
            video_codec,
            audio_codec,
            video_idx: 0,
            audio_idx: 1,
            fu_buffer: Vec::new(),
            fu_started: false,
            pre_video_ts: 0,
            audio_timeline: Duration::from_secs(0),
            audio_timescale: 8000,
            packet_queue: Vec::new(),
        }
    }

    pub fn set_channels(&mut self, video: u8, audio: u8) {
        self.video_channel = video;
        self.audio_channel = audio;
    }

    pub fn feed_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        self.parse_buffer();
    }

    pub fn poll_packet(&mut self) -> Option<Packet> {
        if !self.packet_queue.is_empty() {
            Some(self.packet_queue.remove(0))
        } else {
            None
        }
    }

    fn parse_buffer(&mut self) {
        let mut pos = 0;
        
        while pos + 4 <= self.buffer.len() {
            if self.buffer[pos] == 0x24 { // interleaved binary header symbol '$'
                let channel = self.buffer[pos + 1];
                let length = ((self.buffer[pos + 2] as usize) << 8) | (self.buffer[pos + 3] as usize);
                
                if pos + 4 + length > self.buffer.len() {
                    break; // Wait for more data
                }
                
                // Copy slice to avoid immutable borrow overlap
                let rtp_packet = self.buffer[pos + 4 .. pos + 4 + length].to_vec();
                self.demux_rtp(channel, &rtp_packet);
                pos += 4 + length;
            } else {
                // Seek to next potential header
                pos += 1;
            }
        }
        
        if pos > 0 {
            self.buffer.drain(0..pos);
        }
    }

    fn demux_rtp(&mut self, channel: u8, payload: &[u8]) {
        if payload.len() < 12 {
            return;
        }

        // RTP Header parsing
        let version_flags = payload[0];
        let cc = (version_flags & 0x0f) as usize;
        let has_extension = (version_flags & 0x10) != 0;
        
        let mut offset = 12 + cc * 4;
        
        if has_extension && offset + 4 <= payload.len() {
            let ext_len = (((payload[offset + 2] as usize) << 8) | (payload[offset + 3] as usize)) * 4;
            offset += 4 + ext_len;
        }

        if offset >= payload.len() {
            return;
        }

        let timestamp = ((payload[4] as u64) << 24)
            | ((payload[5] as u64) << 16)
            | ((payload[6] as u64) << 8)
            | (payload[7] as u64);

        let rtp_data = payload[offset..].to_vec();

        if channel == self.video_channel {
            self.handle_video_rtp(timestamp, &rtp_data);
        } else if channel == self.audio_channel {
            self.handle_audio_rtp(timestamp, &rtp_data);
        }
    }

    fn handle_video_rtp(&mut self, timestamp: u64, rtp_data: &[u8]) {
        if rtp_data.is_empty() {
            return;
        }

        if self.pre_video_ts == 0 {
            self.pre_video_ts = timestamp;
        }

        let duration_ms = if timestamp >= self.pre_video_ts {
            (timestamp - self.pre_video_ts) as f64 / 90.0
        } else {
            0.0
        };

        match self.video_codec {
            CodecType::H264 => {
                let nalu_type = rtp_data[0] & 0x1f;
                match nalu_type {
                    1..=5 => {
                        // Single NAL unit packet
                        self.push_video_packet(rtp_data, nalu_type == 5, duration_ms, timestamp);
                    }
                    28 => {
                        // FU-A Fragmented NAL unit
                        let fu_indicator = rtp_data[0];
                        let fu_header = rtp_data[1];
                        let is_start = (fu_header & 0x80) != 0;
                        let is_end = (fu_header & 0x40) != 0;
                        let inner_type = fu_header & 0x1f;

                        if is_start {
                            self.fu_started = true;
                            self.fu_buffer.clear();
                            // Reconstruct NAL header
                            self.fu_buffer.push((fu_indicator & 0xe0) | inner_type);
                        }

                        if self.fu_started {
                            self.fu_buffer.extend_from_slice(&rtp_data[2..]);
                            if is_end {
                                self.fu_started = false;
                                let is_key = inner_type == 5;
                                // Clone fu_buffer to pass to push_video_packet
                                let fu_data = self.fu_buffer.clone();
                                self.push_video_packet(&fu_data, is_key, duration_ms, timestamp);
                            }
                        }
                    }
                    _ => {}
                }
            }
            CodecType::H265 => {
                let nalu_type = (rtp_data[0] >> 1) & 0x3f;
                match nalu_type {
                    0..=21 => {
                        // Single NAL unit packet
                        let is_key = nalu_type == 19 || nalu_type == 20; // IDR slices
                        self.push_video_packet(rtp_data, is_key, duration_ms, timestamp);
                    }
                    49 => {
                        // Fragmented NAL unit (FU)
                        let payload_hdr0 = rtp_data[0];
                        let payload_hdr1 = rtp_data[1];
                        let fu_header = rtp_data[2];
                        let is_start = (fu_header & 0x80) != 0;
                        let is_end = (fu_header & 0x40) != 0;
                        let inner_type = fu_header & 0x3f;

                        if is_start {
                            self.fu_started = true;
                            self.fu_buffer.clear();
                            // Reconstruct HEVC NAL header (2 bytes)
                            let h0 = (payload_hdr0 & 0x81) | (inner_type << 1);
                            let h1 = payload_hdr1;
                            self.fu_buffer.push(h0);
                            self.fu_buffer.push(h1);
                        }

                        if self.fu_started {
                            self.fu_buffer.extend_from_slice(&rtp_data[3..]);
                            if is_end {
                                self.fu_started = false;
                                let is_key = inner_type == 19 || inner_type == 20;
                                // Clone fu_buffer to pass to push_video_packet
                                let fu_data = self.fu_buffer.clone();
                                self.push_video_packet(&fu_data, is_key, duration_ms, timestamp);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        self.pre_video_ts = timestamp;
    }

    fn push_video_packet(&mut self, nal: &[u8], is_key: bool, duration_ms: f64, timestamp: u64) {
        // Prepend AVCC length prefix (4 bytes big-endian) to match zenovdk/v4cctv frame format
        let len = nal.len() as u32;
        let mut data = len.to_be_bytes().to_vec();
        data.extend_from_slice(nal);

        let time_ms = (timestamp as f64) / 90.0;
        
        self.packet_queue.push(Packet {
            idx: self.video_idx,
            is_key_frame: is_key,
            time: Duration::from_millis(time_ms as u64),
            composition_time: Duration::from_millis(1),
            duration: Duration::from_millis(duration_ms as u64),
            data: bytes::Bytes::from(data),
        });
    }

    fn handle_audio_rtp(&mut self, _timestamp: u64, rtp_data: &[u8]) {
        if rtp_data.is_empty() {
            return;
        }

        match self.audio_codec {
            CodecType::AAC => {
                if rtp_data.len() < 4 {
                    return;
                }
                // AAC AU header parsing (RFC 3640)
                let au_headers_length = ((rtp_data[0] as u16) << 8) | (rtp_data[1] as u16);
                let au_headers_count = (au_headers_length >> 4) as usize;
                
                let frames_offset = 2 + au_headers_count * 2;
                if rtp_data.len() < frames_offset {
                    return;
                }
                
                let au_headers = &rtp_data[2..frames_offset];
                let mut frames_payload = &rtp_data[frames_offset..];
                
                let duration = Duration::from_nanos(((1024.0 / self.audio_timescale as f64) * 1_000_000_000.0) as u64);

                for i in 0..au_headers_count {
                    let idx = i * 2;
                    let au_header = ((au_headers[idx] as u16) << 8) | (au_headers[idx + 1] as u16);
                    let frame_size = (au_header >> 3) as usize;
                    
                    if frames_payload.len() < frame_size {
                        break;
                    }
                    
                    let mut frame = &frames_payload[..frame_size];
                    
                    // If frame starts with ADTS syncword (0xfff), strip ADTS header (7 bytes)
                    if frame.len() > 7 && frame[0] == 0xff && (frame[1] & 0xf0) == 0xf0 {
                        frame = &frame[7..];
                    }

                    self.audio_timeline += duration;

                    self.packet_queue.push(Packet {
                        idx: self.audio_idx,
                        is_key_frame: false,
                        time: self.audio_timeline,
                        composition_time: Duration::from_millis(1),
                        duration,
                        data: bytes::Bytes::copy_from_slice(frame),
                    });
                    
                    frames_payload = &frames_payload[frame_size..];
                }
            }
            CodecType::PcmAlaw | CodecType::PcmMulaw => {
                let duration = Duration::from_secs(rtp_data.len() as u64) / self.audio_timescale;
                self.audio_timeline += duration;
                
                self.packet_queue.push(Packet {
                    idx: self.audio_idx,
                    is_key_frame: false,
                    time: self.audio_timeline,
                    composition_time: Duration::from_millis(1),
                    duration,
                    data: bytes::Bytes::copy_from_slice(rtp_data),
                });
            }
            _ => {}
        }
    }
}
