# Protocolo Ares

Notas sobre el protocolo binario de Ares Galaxy tal como lo implementa Astra.

> **Fuente**: sb0t (C#/.NET) → reimplementación en Rust. El protocolo es
> el mismo; los opcodes y formatos son **1:1 con el original**.

## Stack

```
┌────────────────────────────────────┐
│  Cliente Ares (oficial o custom)   │
└──────────┬─────────────────────────┘
           │ TCP (puerto 5009 default)
           │
           ├── MSG_LINK_PROTO (0xFB) → Link Server
           ├── HTTP/1.1 GET (Upgrade) → WebSocket
           └── Otro byte              → Ares TCP
           
           │ UDP (puerto 5009 default)
           └── UdpMsg (9 variantes) → Room Search
```

**Astra multiplexa** TCP/WS/Link en un solo puerto mirando el primer byte
(`astra/src/main.rs:classify_connection`).

## Ares TCP (52 mensajes)

Definidos en `crates/proto-ares/src/messages.rs`. Estructura de cada
paquete:

```text
┌──────────┬─────────────────────┐
│ u8 opcode│ payload (opaque)    │
└──────────┴─────────────────────┘
```

Sin length prefix (cada opcode define su propio formato).

### Clientes → servidor (subset importante)

| Opcode | Nombre | Descripción |
|---:|---|---|
| 1 | `ClientRelogin` | Reconexión |
| 2 | `ClientLogin` | Handshake inicial (25+ campos) |
| 4 | `ClientUpdateStatus` | Cambio de status (browsable, age...) |
| 9 | `Avatar` | Imagen del avatar |
| 10 | `Public` | Mensaje público |
| 11 | `Emote` | Emote (/me) |
| 13 | `PersonalMessage` | Cambio de PM personal |
| 14 | `FastPing` | Keep-alive (responder con FastPing vacío) |
| 25 | `Pmt` | Mensaje privado |
| 50 | `ClientBrowse` | Browse de archivos |

### Servidor → clientes

| Opcode | Nombre | Descripción |
|---:|---|---|
| 0 | `ServerError` | Error genérico + cierre |
| 3 | `ServerLoginAck` | ACK de login (ok/rechazado) |
| 5 | `ServerUpdateUserStatus` | Broadcast de status update |
| 6 | `ServerRedirect` | Redirigir a otra sala |
| 8 | `ServerEcho` | Echo |
| 20 | `ServerJoin` | User joined (broadcast) |
| 22 | `ServerPart` | User parted (broadcast) |
| 26 | `ServerIsIgnoringYou` | El target te ignora |
| 27 | `ServerOfflineUser` | El target está offline |
| 30 | `ServerChannelUserList` | Item de userlist |
| 32 | `ServerTopicFirst` | Topic de la sala |
| 35 | `ServerChannelUserListEnd` | Fin de userlist |
| 44 | `ServerNoSuch` | User no encontrado |
| 75 | `ServerOpChange` | Nivel de op |

## UDP (9 mensajes)

Definidos en `crates/proto-ares/src/messages.rs` (variantes `UdpMsg`).
Se usa para **room search** (descubrimiento de otras salas Ares).

| Opcode | Nombre | Descripción |
|---:|---|---|
| 2 | `SENDINFO` | "¿Estás vivo?" |
| 3 | `ACKINFO` | Respuesta con info del server |
| 11 | `ADDIPS` | Compartir lista de nodos |
| 12 | `ACKIPS` | ACK de ADDIPS |
| 21 | `SENDNODES` | Nodos Ares 2.x |
| 22 | `ACKNODES` | ACK de SENDNODES |
| 31 | `WANTCHECKFIREWALL` | Pedir check de firewall |
| 32 | `READYTOCHECKFIREWALL` | Listo para check |
| 33 | `PROCEEDCHECKFIREWALL` | Proceder con check |
| 34 | `CHECKFIREWALLBUSY` | "Estoy ocupado, check más tarde" |

Los 4 últimos (firewall) son **stubs** en Astra — no se implementa el
check real.

## Link protocol (35 mensajes)

Definidos en `crates/link/src/protocol.rs`. Usado para conectar dos
Astra servers (Hub ↔ Leaf) y compartir la userlist.

Wire format:
```text
┌──────────┬──────────┬──────────────┐
│ u16 len  │ u8 opcode│ payload      │
└──────────┴──────────┴──────────────┘
```

Todos los strings son **null-terminated** (estilo C).

## Reader/Writer

`crates/proto-ares/src/reader.rs` y `writer.rs` proveen:

```rust
let mut r = PacketReader::new(&bytes);
let opcode = r.read_u8()?;
let name   = r.read_string()?;   // u32 len (LE) + UTF-8
let age    = r.read_u8()?;

let mut w = PacketWriter::with_msg(TcpMsg::ServerLoginAck);
w.write_string("Alice")?;
w.write_u16(42)?;
w.write_u8(30)?;
let bytes = w.as_bytes();
```

Tipos soportados: `u8`, `u16` (LE), `u32` (LE), `i32` (LE), `u64` (LE),
`string` (u32 len + UTF-8), `guid` (16 bytes), `bool`, `IPv4`.

## Fuzzing

`fuzz/fuzz_targets/fuzz_reader.rs`, `fuzz_writer.rs` y `fuzz_login.rs`
ejercitan los parsers con bytes random. Ver `fuzz/README.md`.
