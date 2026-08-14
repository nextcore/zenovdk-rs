# ⚡ zenovdk-rs
**High-Performance, Runtime-Agnostic Video Development Kit for Rust (Edition 2024)**

`zenovdk-rs` is a lightweight, pure Rust library designed for parsing RTSP stream inputs and muxing them into fragmented MP4 (fMP4/MSE) or MPEG-TS formats for live web streaming. 

---

## 🌟 Why zenovdk-rs?

Traditional video handling libraries are often tightly coupled to async runtimes (like `Tokio`) or depend heavily on external C bindings (like GStreamer). `zenovdk-rs` solves this by introducing a **pure memory-to-memory state machine** architecture.

- **🚀 Engine & Runtime Agnostic**: No Tokio, async-std, or other runtime dependencies. Runs anywhere, from cloud servers to bare-metal embedded IoT and WebAssembly.
- **⚡ Zero-Copy Buffers**: Uses `bytes::Bytes` internally to prevent overhead when broadcasting video packets to thousands of clients.
- **📦 Pure Rust**: 100% Rust code with zero compiler/C-dependency headaches. Easy to cross-compile.
- **🎨 All-in-One**: Packaged with RTSP/RTP parsing, H264/H265 parameter decoding, and fMP4/MPEG-TS muxing out of the box.

---

## ⚙️ Modular Features

- **`codec`**: Sub-byte bitstream parser for H264, H265 (SPS/PPS extraction, Slice-type determination), and AAC ADTS config parsing.
- **`format::rtsp`**: RTP demuxer & packet reassembler (handling H264 FU-A, H265 FU, and AAC payloads).
- **`format::mp4f`**: Fragmented MP4 (ISO BMFF) muxer that generates `ftyp`, `moov`, `moof`, and `mdat` boxes for browser Media Source Extensions (MSE).
- **`format::ts`**: MPEG-TS packetizer (188-byte aligned) with PAT, PMT, and PES headers for HLS streaming.

---

## 🚀 Quickstart

### 1. Add Dependency
Add this to your `Cargo.toml`:
```toml
[dependencies]
zenovdk = { git = "https://github.com/nextcore/zenovdk-rs.git" }
```

### 2. Muxing Packets to fMP4 (Web Player ready)
```rust
use zenovdk::av::CodecType;
use zenovdk::format::mp4f::Mp4Muxer;

fn main() {
    let mut muxer = Mp4Muxer::new();
    
    // Add H264 Video stream with width, height, and extra data (SPS/PPS decoder config)
    muxer.add_stream(CodecType::H264, 1920, 1080, vec![/* SPS/PPS bytes */]);
    
    // Get Init segment for HTML5 video player initialization
    let init_bytes = muxer.get_init();
    
    // Mux incoming RTP packets
    // let (completed, fragment_bytes) = muxer.write_packet(&pkt, 30);
}
```

---

## 📜 License
Licensed under the [MIT License](LICENSE).
