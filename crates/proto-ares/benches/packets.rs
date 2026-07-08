//! Benchmarks del protocolo binario Ares (PacketReader/PacketWriter).
//!
//! Ejecutar con: `cargo bench -p proto-ares`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proto_ares::{PacketReader, PacketWriter};

/// Serializa un paquete estilo login (mix representativo de tipos).
fn build_login_like() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_u8(black_box(0x02)).unwrap();
    w.write_u16_le(black_box(5009)).unwrap();
    w.write_u32_le(black_box(0xDEAD_BEEF)).unwrap();
    w.write_string(black_box("UsuarioDePrueba")).unwrap();
    w.write_string(black_box("Ares 2.4.7")).unwrap();
    w.write_string(black_box("us-east")).unwrap();
    w.write_bool(black_box(true)).unwrap();
    w.write_u16_le(black_box(1234)).unwrap();
    w.into_bytes()
}

fn bench_writer(c: &mut Criterion) {
    c.bench_function("writer_login_like", |b| b.iter(build_login_like));
}

fn bench_reader(c: &mut Criterion) {
    let data = build_login_like();
    c.bench_function("reader_login_like", |b| {
        b.iter(|| {
            let mut r = PacketReader::new(black_box(&data));
            let _ = black_box(r.read_u8().unwrap());
            let _ = black_box(r.read_u16_le().unwrap());
            let _ = black_box(r.read_u32_le().unwrap());
            let _ = black_box(r.read_string().unwrap());
            let _ = black_box(r.read_string().unwrap());
            let _ = black_box(r.read_string().unwrap());
            let _ = black_box(r.read_bool().unwrap());
            let _ = black_box(r.read_u16_le().unwrap());
        })
    });
}

fn bench_writer_large_string(c: &mut Criterion) {
    let text = "x".repeat(300);
    c.bench_function("writer_topic_300_chars", |b| {
        b.iter(|| {
            let mut w = PacketWriter::new();
            w.write_string(black_box(&text)).unwrap();
            w.into_bytes()
        })
    });
}

criterion_group!(benches, bench_writer, bench_reader, bench_writer_large_string);
criterion_main!(benches);
