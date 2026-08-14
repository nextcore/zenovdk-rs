use zenovdk::av::{Packet, CodecType};
use zenovdk::codec::h264::{parse_sps, SPSInfo};
use zenovdk::format::mp4f::Mp4Muxer;
use zenovdk::format::ts::TsMuxer;
use std::time::Duration;
use bytes::Bytes;

#[test]
fn test_h264_sps_parser() {
    // Mock SPS byte sequence for H264 (Profile: Baseline, 640x480 resolution, 30fps)
    let sps_bytes = [
        0x67, 0x42, 0x00, 0x1e, 0x96, 0x54, 0x02, 0x80,
        0x2d, 0x80, 0x00, 0x00, 0x03, 0x00, 0x80, 0x00,
        0x00, 0x1e, 0x07, 0x8c, 0x18, 0xcb
    ];

    let info = parse_sps(&sps_bytes);
    assert!(info.is_ok(), "SPS Parsing failed: {:?}", info.err());
    
    let sps = info.unwrap();
    assert_eq!(sps.width, 1280);
    assert_eq!(sps.height, 720);
}

#[test]
fn test_fmp4_muxer_init_and_write() {
    let mut muxer = Mp4Muxer::new();
    
    // Add video stream
    muxer.add_stream(CodecType::H264, 1920, 1080, vec![1, 2, 3, 4]);
    
    // Generate init segment
    let init_bytes = muxer.get_init();
    assert!(!init_bytes.is_empty(), "Init segment must not be empty");
    
    // First 4 bytes must be the length, next 4 bytes must be "ftyp"
    assert_eq!(&init_bytes[4..8], b"ftyp");
    
    // Write packet and verify segment completion
    let pkt = Packet {
        idx: 0,
        is_key_frame: true,
        time: Duration::from_millis(0),
        composition_time: Duration::from_millis(0),
        duration: Duration::from_millis(33),
        data: Bytes::from(vec![0x00, 0x00, 0x00, 0x05, 0x65, 0x10, 0x20, 0x30, 0x40]),
    };
    
    // Mux packet (forcing finalization with max_frames = 0)
    let (completed, fragment) = muxer.write_packet(&pkt, 0);
    assert!(completed, "Should finalize fragment on keyframe when limit exceeded");
    assert!(!fragment.is_empty(), "Fragment segment must not be empty");
    
    // Fragment header must start with "moof"
    assert_eq!(&fragment[4..8], b"moof");
}

#[test]
fn test_mpeg_ts_muxer_alignment() {
    let mut muxer = TsMuxer::new();
    muxer.add_stream(0, CodecType::H264);
    
    let pat_pmt = muxer.get_pat_pmt();
    assert_eq!(pat_pmt.len() % 188, 0, "PAT/PMT segment size must be a multiple of 188 bytes");
    
    let pkt = Packet {
        idx: 0,
        is_key_frame: true,
        time: Duration::from_millis(0),
        composition_time: Duration::from_millis(0),
        duration: Duration::from_millis(33),
        data: Bytes::from(vec![0x65, 0x10, 0x20, 0x30, 0x40]),
    };
    
    let ts_segment = muxer.write_packet(&pkt);
    assert!(!ts_segment.is_empty());
    assert_eq!(ts_segment.len() % 188, 0, "TS payload packet size must be a multiple of 188 bytes");
}
