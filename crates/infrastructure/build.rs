fn main() {
    // Uses a pre-compiled protoc binary bundled via protoc-bin-vendored
    // rather than requiring a system-installed `protoc` — that would need
    // its own install step on every GitHub Actions runner platform
    // (macOS/Windows/Linux) this project already builds for, which is
    // exactly the kind of extra native-toolchain dependency that's
    // caused real build friction in this project before.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().expect("vendored protoc binary not found for this platform"));
    prost_build::compile_protos(&["proto/market_data_feed_v3.proto"], &["proto/"])
        .expect("failed to compile market_data_feed_v3.proto — check the schema file is present and valid");
    println!("cargo:rerun-if-changed=proto/market_data_feed_v3.proto");
}
