use crate::av::{Packet, CodecType};

pub struct Stream {
    pub codec_type: CodecType,
    pub track_id: u32,
    pub time_scale: u32,
    pub width: u32,
    pub height: u32,
    pub codec_data: Vec<u8>, // Extra data (SPS/PPS record or AudioConfig)
    pub last_dts: u64,
}

pub struct Mp4Muxer {
    pub streams: Vec<Stream>,
    pub fragment_index: u32,
    pub sample_index: u32,
    pub moof_entries: Vec<TrackFragRunEntry>,
    pub mdat_buffer: Vec<u8>,
}

pub struct TrackFragRunEntry {
    pub duration: u32,
    pub size: u32,
    pub cts: u32,
    pub is_keyframe: bool,
}

impl Mp4Muxer {
    pub fn new() -> Self {
        Self {
            streams: Vec::new(),
            fragment_index: 0,
            sample_index: 0,
            moof_entries: Vec::new(),
            mdat_buffer: Vec::new(),
        }
    }

    pub fn add_stream(&mut self, codec_type: CodecType, width: u32, height: u32, codec_data: Vec<u8>) {
        let track_id = (self.streams.len() + 1) as u32;
        let time_scale = match codec_type {
            CodecType::H264 | CodecType::H265 => 90000,
            CodecType::AAC => 48000, // standard default
            _ => 1000,
        };
        self.streams.push(Stream {
            codec_type,
            track_id,
            time_scale,
            width,
            height,
            codec_data,
            last_dts: 0,
        });
    }

    // Generate Init segment (ftyp + moov)
    pub fn get_init(&self) -> Vec<u8> {
        let mut f = Vec::new();

        // 1. Write ftyp box
        let ftyp_data = [
            0x00, 0x00, 0x00, 0x18, // size (24 bytes)
            0x66, 0x74, 0x79, 0x70, // "ftyp"
            0x69, 0x73, 0x6f, 0x36, // major_brand "iso6"
            0x00, 0x00, 0x00, 0x01, // minor_version 1
            0x69, 0x73, 0x6f, 0x36, // compatible_brands: "iso6"
            0x64, 0x61, 0x73, 0x68, // compatible_brands: "dash"
        ];
        f.extend_from_slice(&ftyp_data);

        // 2. Write moov box skeleton (simple raw builder to avoid heavy external crates)
        let mut moov = Vec::new();

        // Write mvhd
        let mvhd = [
            0x00, 0x00, 0x00, 0x6c, 0x6d, 0x76, 0x68, 0x64, // size & "mvhd"
            0x00, 0x00, 0x00, 0x00, // version & flags
            0x00, 0x00, 0x00, 0x00, // creation_time
            0x00, 0x00, 0x00, 0x00, // modification_time
            0x00, 0x00, 0x03, 0xe8, // timescale (1000)
            0x00, 0x00, 0x00, 0x00, // duration
            0x00, 0x01, 0x00, 0x00, // rate
            0x01, 0x00, 0x00, 0x00, // volume
            0,0,0,0, 0,0,0,0,
            0x00, 0x01, 0x00, 0x00, 0,0,0,0, 0,0,0,0,
            0,0,0,0, 0x00, 0x01, 0x00, 0x00, 0,0,0,0,
            0,0,0,0, 0,0,0,0, 0x40, 0x00, 0x00, 0x00, // matrix
            0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,
            0x00, 0x00, 0x00, 0x03, // next_track_id
        ];
        moov.extend_from_slice(&mvhd);

        // Write tracks (trak)
        for stream in &self.streams {
            let mut trak = Vec::new();
            
            // tkhd (track header)
            let mut tkhd = vec![0, 0, 0, 92];
            tkhd.extend_from_slice(b"tkhd");
            tkhd.extend_from_slice(&[0, 0, 0, 7]); // flags: enabled, in movie, in preview
            tkhd.extend_from_slice(&[0; 8]); // creation & modification time
            tkhd.extend_from_slice(&stream.track_id.to_be_bytes()); // track_id
            tkhd.extend_from_slice(&[0; 4]); // reserved
            tkhd.extend_from_slice(&[0; 4]); // duration
            tkhd.extend_from_slice(&[0; 8]); // reserved
            tkhd.extend_from_slice(&[0; 2]); // layer
            tkhd.extend_from_slice(&[0; 2]); // alternate_group
            tkhd.extend_from_slice(&[0x01, 0x00]); // volume (1.0)
            tkhd.extend_from_slice(&[0; 2]); // reserved
            tkhd.extend_from_slice(&[
                0x00, 0x01, 0x00, 0x00, 0,0,0,0, 0,0,0,0,
                0,0,0,0, 0x00, 0x01, 0x00, 0x00, 0,0,0,0,
                0,0,0,0, 0,0,0,0, 0x40, 0x00, 0x00, 0x00
            ]); // matrix
            tkhd.extend_from_slice(&((stream.width as i32) << 16).to_be_bytes()); // width
            tkhd.extend_from_slice(&((stream.height as i32) << 16).to_be_bytes()); // height
            trak.extend_from_slice(&tkhd);

            // mdia (media box)
            let mut mdia = Vec::new();
            
            // mdhd
            let mut mdhd = vec![0, 0, 0, 32];
            mdhd.extend_from_slice(b"mdhd");
            mdhd.extend_from_slice(&[0; 12]); // version, flags, creation, mod time
            mdhd.extend_from_slice(&stream.time_scale.to_be_bytes()); // timescale
            mdhd.extend_from_slice(&[0; 4]); // duration
            mdhd.extend_from_slice(&[0x55, 0xc4, 0, 0]); // language und
            mdia.extend_from_slice(&mdhd);

            // hdlr
            let sub_type = match stream.codec_type {
                CodecType::H264 | CodecType::H265 => b"vide",
                CodecType::AAC => b"soun",
                _ => b"hint",
            };
            let mut hdlr = vec![0, 0, 0, 37];
            hdlr.extend_from_slice(b"hdlr");
            hdlr.extend_from_slice(&[0; 8]); // version, flags, component type (8 bytes)
            hdlr.extend_from_slice(sub_type); // (4 bytes, e.g. "vide")
            hdlr.extend_from_slice(&[0; 12]); // manufacturer/flags/mask (12 bytes)
            hdlr.extend_from_slice(b"GG\x00\x00\x00"); // name (5 bytes)
            mdia.extend_from_slice(&hdlr);

            // minf (media info)
            let mut minf = Vec::new();
            
            match stream.codec_type {
                CodecType::H264 | CodecType::H265 => {
                    minf.extend_from_slice(b"\x00\x00\x00\x14vmhd\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00"); // vmhd
                }
                CodecType::AAC => {
                    minf.extend_from_slice(b"\x00\x00\x00\x10smhd\x00\x00\x00\x00\x00\x00\x00\x00"); // smhd
                }
                _ => {}
            }

            // dinf & dref
            let dref = [
                0x00, 0x00, 0x00, 0x24, 0x64, 0x69, 0x6e, 0x66, // size (36) & "dinf"
                0x00, 0x00, 0x00, 0x1c, 0x64, 0x72, 0x65, 0x66, // size (28) & "dref"
                0x00, 0x00, 0x00, 0x00, // version, flags
                0x00, 0x00, 0x00, 0x01, // entry count
                0x00, 0x00, 0x00, 0x0c, 0x75, 0x72, 0x6c, 0x20, // size (12) & "url "
                0x00, 0x00, 0x00, 0x01, // flags (self-contained)
            ];
            minf.extend_from_slice(&dref);

            // stbl (sample table)
            let mut stbl = Vec::new();
            
            // stsd (sample description)
            let mut stsd_payload = Vec::new();
            stsd_payload.extend_from_slice(&[0, 0, 0, 0]); // version, flags
            stsd_payload.extend_from_slice(&[0, 0, 0, 1]); // entry count

            match stream.codec_type {
                CodecType::H264 => {
                    let mut avc1 = Vec::new();
                    avc1.extend_from_slice(&(94 + stream.codec_data.len() as u32).to_be_bytes()); // length
                    avc1.extend_from_slice(b"avc1");
                    avc1.extend_from_slice(&[0; 6]); // reserved
                    avc1.extend_from_slice(&[0, 1]); // data ref index
                    avc1.extend_from_slice(&[0; 16]); // pre-defined, reserved
                    avc1.extend_from_slice(&(stream.width as u16).to_be_bytes()); // width
                    avc1.extend_from_slice(&(stream.height as u16).to_be_bytes()); // height
                    avc1.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]); // horiz resolution 72 dpi
                    avc1.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]); // vert resolution 72 dpi
                    avc1.extend_from_slice(&[0; 4]); // reserved
                    avc1.extend_from_slice(&[0, 1]); // frame count (1)
                    avc1.extend_from_slice(&[0; 32]); // compressor name
                    avc1.extend_from_slice(&[0, 24]); // depth
                    avc1.extend_from_slice(&[0xff, 0xff]); // pre-defined
                    
                    // avcc configuration box
                    let mut avcc = Vec::new();
                    avcc.extend_from_slice(&((8 + stream.codec_data.len()) as u32).to_be_bytes());
                    avcc.extend_from_slice(b"avcC");
                    avcc.extend_from_slice(&stream.codec_data);
                    avc1.extend_from_slice(&avcc);
                    stsd_payload.extend_from_slice(&avc1);
                }
                CodecType::H265 => {
                    let mut hvc1 = Vec::new();
                    hvc1.extend_from_slice(&(94 + stream.codec_data.len() as u32).to_be_bytes()); // length
                    hvc1.extend_from_slice(b"hvc1");
                    hvc1.extend_from_slice(&[0; 6]); // reserved
                    hvc1.extend_from_slice(&[0, 1]); // data ref index
                    hvc1.extend_from_slice(&[0; 16]); // pre-defined, reserved
                    hvc1.extend_from_slice(&(stream.width as u16).to_be_bytes());
                    hvc1.extend_from_slice(&(stream.height as u16).to_be_bytes());
                    hvc1.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]);
                    hvc1.extend_from_slice(&[0x00, 0x48, 0x00, 0x00]);
                    hvc1.extend_from_slice(&[0; 4]);
                    hvc1.extend_from_slice(&[0, 1]);
                    hvc1.extend_from_slice(&[0; 32]);
                    hvc1.extend_from_slice(&[0, 24]);
                    hvc1.extend_from_slice(&[0xff, 0xff]);

                    // hvcc configuration box
                    let mut hvcc = Vec::new();
                    hvcc.extend_from_slice(&((8 + stream.codec_data.len()) as u32).to_be_bytes());
                    hvcc.extend_from_slice(b"hvcC");
                    hvcc.extend_from_slice(&stream.codec_data);
                    hvc1.extend_from_slice(&hvcc);
                    stsd_payload.extend_from_slice(&hvc1);
                }
                CodecType::AAC => {
                    let mut mp4a = Vec::new();
                    mp4a.extend_from_slice(&(44 + stream.codec_data.len() as u32).to_be_bytes()); // length
                    mp4a.extend_from_slice(b"mp4a");
                    mp4a.extend_from_slice(&[0; 6]); // reserved
                    mp4a.extend_from_slice(&[0, 1]); // data ref index
                    mp4a.extend_from_slice(&[0; 8]); // reserved
                    mp4a.extend_from_slice(&[0, 2]); // channels (2)
                    mp4a.extend_from_slice(&[0, 16]); // sample size (16)
                    mp4a.extend_from_slice(&[0; 4]); // reserved
                    mp4a.extend_from_slice(&[0xbb, 0x80, 0x00, 0x00]); // sample rate 48000
                    
                    // esds configuration box
                    let mut esds = Vec::new();
                    esds.extend_from_slice(&((8 + stream.codec_data.len()) as u32).to_be_bytes());
                    esds.extend_from_slice(b"esds");
                    esds.extend_from_slice(&stream.codec_data);
                    mp4a.extend_from_slice(&esds);
                    stsd_payload.extend_from_slice(&mp4a);
                }
                _ => {}
            }
            
            let stsd_len = (stsd_payload.len() + 8) as u32;
            let mut stsd_box = stsd_len.to_be_bytes().to_vec();
            stsd_box.extend_from_slice(b"stsd");
            stsd_box.extend_from_slice(&stsd_payload);
            stbl.extend_from_slice(&stsd_box);

            // Empty table placeholders (stts, stsc, stss, stco, stsz)
            stbl.extend_from_slice(b"\x00\x00\x00\x10stts\x00\x00\x00\x00\x00\x00\x00\x00"); // stts
            stbl.extend_from_slice(b"\x00\x00\x00\x10stsc\x00\x00\x00\x00\x00\x00\x00\x00"); // stsc
            stbl.extend_from_slice(b"\x00\x00\x00\x10stss\x00\x00\x00\x00\x00\x00\x00\x00"); // stss
            stbl.extend_from_slice(b"\x00\x00\x00\x10stco\x00\x00\x00\x00\x00\x00\x00\x00"); // stco
            stbl.extend_from_slice(b"\x00\x00\x00\x14stsz\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"); // stsz

            let stbl_len = (stbl.len() + 8) as u32;
            let mut stbl_box = stbl_len.to_be_bytes().to_vec();
            stbl_box.extend_from_slice(b"stbl");
            stbl_box.extend_from_slice(&stbl);
            minf.extend_from_slice(&stbl_box);

            let minf_len = (minf.len() + 8) as u32;
            let mut minf_box = minf_len.to_be_bytes().to_vec();
            minf_box.extend_from_slice(b"minf");
            minf_box.extend_from_slice(&minf);
            mdia.extend_from_slice(&minf_box);

            let mdia_len = (mdia.len() + 8) as u32;
            let mut mdia_box = mdia_len.to_be_bytes().to_vec();
            mdia_box.extend_from_slice(b"mdia");
            mdia_box.extend_from_slice(&mdia);
            trak.extend_from_slice(&mdia_box);

            let trak_len = (trak.len() + 8) as u32;
            let mut trak_box = trak_len.to_be_bytes().to_vec();
            trak_box.extend_from_slice(b"trak");
            trak_box.extend_from_slice(&trak);
            moov.extend_from_slice(&trak_box);
        }

        // Write mvex (movie extends box for fragmented MP4 support)
        let mut mvex = Vec::new();
        mvex.extend_from_slice(b"\x00\x00\x00\x10mehd\x00\x00\x00\x00\x00\x00\x00\x00"); // mehd
        for stream in &self.streams {
            let mut trex = vec![0, 0, 0, 32];
            trex.extend_from_slice(b"trex");
            trex.extend_from_slice(&[0, 0, 0, 0]); // version & flags
            trex.extend_from_slice(&stream.track_id.to_be_bytes()); // track_id
            trex.extend_from_slice(&[0, 0, 0, 1]); // default_sample_description_index
            trex.extend_from_slice(&[0, 0, 0, 0]); // default_sample_duration
            trex.extend_from_slice(&[0, 0, 0, 0]); // default_sample_size
            trex.extend_from_slice(&[0, 0, 0, 0]); // default_sample_flags
            mvex.extend_from_slice(&trex);
        }
        
        let mvex_len = (mvex.len() + 8) as u32;
        let mut mvex_box = mvex_len.to_be_bytes().to_vec();
        mvex_box.extend_from_slice(b"mvex");
        mvex_box.extend_from_slice(&mvex);
        moov.extend_from_slice(&mvex_box);

        let moov_len = (moov.len() + 8) as u32;
        let mut moov_box = moov_len.to_be_bytes().to_vec();
        moov_box.extend_from_slice(b"moov");
        moov_box.extend_from_slice(&moov);
        f.extend_from_slice(&moov_box);

        f
    }

    // Mux packet into moof + mdat fragment
    pub fn write_packet(&mut self, pkt: &Packet, max_frames: u32) -> (bool, Vec<u8>) {
        let stream_idx = pkt.idx as usize;
        if stream_idx >= self.streams.len() {
            return (false, Vec::new());
        }

        let is_keyframe = pkt.is_key_frame;
        let timescale = self.streams[stream_idx].time_scale as f64;
        let raw_duration = pkt.duration.as_secs_f64() * timescale;
        let cts = pkt.composition_time.as_secs_f64() * timescale;

        if self.sample_index == 0 {
            // Pre-allocate mdat header placeholder (length will be written on finalize)
            self.mdat_buffer = vec![0, 0, 0, 0, 0x6d, 0x64, 0x61, 0x74];
        }

        self.moof_entries.push(TrackFragRunEntry {
            duration: raw_duration as u32,
            size: pkt.data.len() as u32,
            cts: cts as u32,
            is_keyframe,
        });

        self.mdat_buffer.extend_from_slice(&pkt.data);
        self.sample_index += 1;

        // If fragment limit is reached or we got a new keyframe after some frames, wrap it up
        if self.sample_index > max_frames && is_keyframe {
            let result = self.finalize(pkt.idx);
            return (true, result);
        }

        (false, Vec::new())
    }

    fn finalize(&mut self, idx: i8) -> Vec<u8> {
        let stream = &mut self.streams[idx as usize];
        let track_id = stream.track_id;
        let seq_num = self.fragment_index + 1;
        self.fragment_index += 1;

        // 1. Build moof box
        let mut moof = Vec::new();

        // mfhd
        let mut mfhd = vec![0, 0, 0, 16];
        mfhd.extend_from_slice(b"mfhd");
        mfhd.extend_from_slice(&[0, 0, 0, 0]); // version, flags
        mfhd.extend_from_slice(&seq_num.to_be_bytes()); // seq number
        moof.extend_from_slice(&mfhd);

        // traf (track fragment)
        let mut traf = Vec::new();
        
        // tfhd
        let mut tfhd = vec![0, 0, 0, 20];
        tfhd.extend_from_slice(b"tfhd");
        tfhd.extend_from_slice(&[0, 0x02, 0x00, 0x20]); // flags: default-base-is-moof, default-sample-flags
        tfhd.extend_from_slice(&track_id.to_be_bytes());
        tfhd.extend_from_slice(&[0x01, 0x01, 0x00, 0x00]); // default flags
        traf.extend_from_slice(&tfhd);

        // tfdt (decode time)
        let mut tfdt = vec![0, 0, 0, 20];
        tfdt.extend_from_slice(b"tfdt");
        tfdt.extend_from_slice(&[1, 0, 0, 0]); // version 1
        tfdt.extend_from_slice(&stream.last_dts.to_be_bytes());
        traf.extend_from_slice(&tfdt);

        // trun
        let mut trun = Vec::new();
        trun.extend_from_slice(&[0, 0x00, 0x0b, 0x05]); // version, flags: data-offset, first-sample-flags, size, duration, cts present
        trun.extend_from_slice(&(self.moof_entries.len() as u32).to_be_bytes());
        
        // Data offset placeholder (will be filled later)
        let data_offset_idx = trun.len();
        trun.extend_from_slice(&[0, 0, 0, 0]);
        trun.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // first sample flags (keyframe)

        let mut total_duration = 0;
        for entry in &self.moof_entries {
            trun.extend_from_slice(&entry.duration.to_be_bytes());
            trun.extend_from_slice(&entry.size.to_be_bytes());
            trun.extend_from_slice(&entry.cts.to_be_bytes());
            
            total_duration += entry.duration as u64;
        }
        
        let trun_len = (trun.len() + 8) as u32;
        let mut trun_box = trun_len.to_be_bytes().to_vec();
        trun_box.extend_from_slice(b"trun");
        trun_box.extend_from_slice(&trun);
        traf.extend_from_slice(&trun_box);

        let traf_len = (traf.len() + 8) as u32;
        let mut traf_box = traf_len.to_be_bytes().to_vec();
        traf_box.extend_from_slice(b"traf");
        traf_box.extend_from_slice(&traf);
        moof.extend_from_slice(&traf_box);

        let moof_len = (moof.len() + 8) as u32;
        let mut moof_box = moof_len.to_be_bytes().to_vec();
        moof_box.extend_from_slice(b"moof");
        moof_box.extend_from_slice(&moof);

        // Update trun data_offset (offset is moof length + 8 bytes of mdat header)
        let data_offset = (moof_box.len() + 8) as u32;
        let offset_pos = 80 + data_offset_idx; // absolute index in moof_box
        if offset_pos + 4 <= moof_box.len() {
            moof_box[offset_pos..offset_pos+4].copy_from_slice(&data_offset.to_be_bytes());
        }

        // Fill mdat size header
        let mdat_len = self.mdat_buffer.len() as u32;
        self.mdat_buffer[0..4].copy_from_slice(&mdat_len.to_be_bytes());

        // Assembly fragment
        let mut fragment = moof_box;
        fragment.extend_from_slice(&self.mdat_buffer);

        // Reset state
        stream.last_dts += total_duration;
        self.sample_index = 0;
        self.moof_entries.clear();
        self.mdat_buffer.clear();

        fragment
    }
}
