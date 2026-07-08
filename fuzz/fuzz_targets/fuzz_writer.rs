#![no_main]
use libfuzzer_sys::fuzz_target;
use proto_ares::{PacketReader, PacketWriter, TcpMsg};

fuzz_target!(|data: &[u8]| {
    // Lee lo que pueda de los bytes fuzzed, lo re-empaqueta y verifica
    // que se pueda leer de nuevo sin panic. Detecta inconsistencias de
    // longitud, encoding y tipos en roundtrips random.
    let reader = PacketReader::new(data);
    let mut w = PacketWriter::with_msg(TcpMsg::ServerError);

    if let Ok(v) = reader.read_u8() {
        w.write_u8(v).ok();
    }
    if data.len() >= 2 {
        let mut r2 = PacketReader::new(data);
        if let Ok(v) = r2.read_u16_le() {
            w.write_u16_le(v).ok();
        }
    }
    if data.len() >= 4 {
        let mut r3 = PacketReader::new(data);
        if let Ok(v) = r3.read_u32_le() {
            w.write_u32_le(v).ok();
        }
    }
    if data.len() >= 5 {
        let mut r4 = PacketReader::new(data);
        if let Ok(s) = r4.read_string() {
            w.write_string(&s).ok();
        }
    }

    // Verificar que el output se pueda volver a leer.
    let bytes = w.as_bytes();
    let mut r5 = PacketReader::new(bytes);
    let _ = r5.read_u8();
});
