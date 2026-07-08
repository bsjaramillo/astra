#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = proto_ares::PacketReader::new(data).read_u8();
});
