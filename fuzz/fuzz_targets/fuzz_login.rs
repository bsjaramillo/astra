#![no_main]
use libfuzzer_sys::fuzz_target;
use server_core::login::parse_login;
use proto_ares::PacketReader;

/// Fuzz target: intenta parsear paquetes de login random sin que el parser
/// panique. Errores legítimos (longitud inválida, UTF-8 malo) son Ok, panics
/// son bugs.
fuzz_target!(|data: &[u8]| {
    // Garantizar al menos 1 byte (el opcode).
    if data.is_empty() {
        return;
    }
    let opcode = data[0];
    if opcode != 2 && opcode != 1 {
        // 2 = ClientLogin, 1 = ClientRelogin. El parser solo acepta esos.
        return;
    }
    // Verificar que el reader no panice con bytes random.
    let mut r = PacketReader::new(data);
    let _ = r.read_u8();
    // parse_login debe retornar Err, no panic.
    let _ = parse_login(data);
});
