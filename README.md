# Astra

> Servidor de chat compatible con **Ares Galaxy**, escrito en **Rust**.

Astra es un servidor de chat multiplataforma que implementa el protocolo binario
de Ares Galaxy — heredero moderno de sb0t. Monta tu propia sala en minutos:
protocolo completo, moderación avanzada, scripting y un panel de administración
web, en cualquier plataforma.

- **Compatibilidad de protocolo**: cualquier cliente Ares oficial se conecta sin cambios.
- **Multiplataforma real**: Linux, Windows, macOS, FreeBSD, Raspberry Pi (ARM).
- **Seguridad por construcción**: memory safety garantizada por el compilador +
  5 capas anti-DDoS + captcha opcional.
- **Despliegue simple**: un solo puerto lógico para TCP (Ares), WebSocket (web),
  Link (multi-servidor) y UDP (room search).
- **Un binario estático**, sin runtime, con miles de conexiones concurrentes.

## Qué incluye

- Protocolo Ares completo (TCP + UDP room search).
- Login, UserPool, persistencia SQLite (bans, cuentas, historial).
- 5 capas de defensa anti-DDoS + captcha para IPs nuevas.
- Chat: público, emote, PM, join/part, topic, vrooms.
- **Moderación completa**: ban, kick, muzzle, niveles de usuario y decenas de
  comandos más, directamente en el chat.
- Clientes web (WebSocket / ib0t / HTML5) en el mismo puerto.
- **Panel de administración web** (`/admin`) con auth por owner password.
- Motor de scripting JS (boa_engine) para plugins de sala.
- Link Hub/Leaf entre servidores con **cifrado AES-256**.
- GeoIP/ASN opcional (MaxMind GeoLite2 o DB-IP Lite) para `/trace` y `asnban`.
- **Aviso de actualizaciones**: chequea el registry cada 6h y avisa por PM a
  los admins/owners cuando hay versión nueva (badge en `/admin`; opt-out con
  `update_check = false`).

---

## Inicio rápido

### Opción A — astra-creator (recomendado)

[astra-creator](https://github.com/bsjaramillo/astra-creator) es una TUI que
crea y administra una o varias salas Astra sobre Docker: genera la config de
cada una, las despliega y maneja su ciclo de vida (start/stop/logs/update)
sin salir de la terminal.

```bash
# Instalar (necesita docker + docker compose):
curl -sSL https://raw.githubusercontent.com/bsjaramillo/astra-creator/main/install.sh | sh

# Abrir la TUI y crear tu sala:
astra-creator /srv/astra-salas
```

En la TUI: `a` agrega una sala (nombre, puerto, owner password y — si quieres
HTTPS — un dominio), `D` la despliega. Cada sala es un contenedor independiente
con su configuración y sus datos.

### Opción B — Docker manual

```bash
git clone https://github.com/bsjaramillo/astra && cd astra
cp astra.toml.example astra.toml   # edita room_name y owner_password
docker compose up -d
```

El server queda escuchando en el puerto **5009** (TCP + UDP).

### Opción C — Binario nativo

```bash
# Requiere Rust 1.75+
cargo build --release
cp astra.toml.example astra.toml   # edita room_name y owner_password
./target/release/astra --config astra.toml
```

---

## Guía para crear tu sala (manual, sin astra-creator)

> Con astra-creator los pasos 1 y 2 los hace la TUI; esta guía es para el
> despliegue manual con el binario o Docker a mano.

1. **Configura** `astra.toml` (copiado de `astra.toml.example`). Lo mínimo:
   ```toml
   room_name = "Mi Sala"
   bot_name  = "MiBot"
   owner_password = "algo-secreto"   # te da nivel Owner y protege /admin
   ```
   > Si `owner_password` queda vacío, el panel de administración se deshabilita.

2. **Arranca** el server:
   ```bash
   ./target/release/astra --config astra.toml
   ```

3. **Abre el puerto** 5009 (TCP **y** UDP) en el firewall del sistema:
   ```bash
   sudo ufw allow 5009/tcp && sudo ufw allow 5009/udp
   ```
   Y **también en el firewall de tu VPS o router**: security groups en AWS,
   reglas de ingreso en Oracle Cloud/GCP, el panel de Hetzner/Vultr/DigitalOcean,
   o el port forwarding del router si lo corres en casa. Es el paso que más se
   olvida: si el proveedor bloquea el puerto, nadie entra aunque `ufw` lo permita.

   Tu IP pública (ej. `curl ifconfig.me`) es la que compartes.

4. **Administra**, de dos formas equivalentes:
   - **Panel web**: `http://<tu-ip>:5009/admin` → ingresa el `owner_password`.
     Gestión de usuarios (ban/kick/muzzle/niveles), bans, flags de sala, greets,
     filtros, edición del config, y una consola de comandos.
   - **Comandos en el chat**: `/login <owner_password>` te hace Owner; después
     `/topic`, `/ban`, `/grant <nick> moderator`, `/help`, etc.

5. **La gente entra**:
   - **Cliente Ares Galaxy**: agregar sala por dirección → `<tu-ip>:5009`.
   - **Navegador**: `http://<tu-ip>:5009/` (cliente de chat web básico).
   - Si dejas `roomsearch = true`, la sala se anuncia en la red de descubrimiento UDP.

---

## Despliegue con HTTPS (reverse proxy)

Astra multiplexa el protocolo binario de Ares y el HTTP/WebSocket **en el mismo
puerto**. Un reverse proxy con TLS solo puede cubrir la parte **web** (cliente
navegador + panel `/admin`); los clientes **Ares** usan TCP binario plano y se
conectan directo al `:5009` (el protocolo Ares no soporta TLS).

**Con astra-creator** (recomendado): pon un dominio en el campo
"Dominio HTTPS" del formulario de la sala (con el DNS apuntando a tu servidor)
y redespliega con `D`. Se levanta automáticamente un [Caddy](https://caddyserver.com)
como reverse proxy con certificados de Let's Encrypt; varias salas pueden tener
cada una su dominio compartiendo el mismo Caddy.

**Manual**: el repo trae el mismo setup listo para usar sin astra-creator:

```bash
# 1. Edita el Caddyfile y pon tu dominio real (chat.midominio.com).
# 2. Apunta el DNS de ese dominio a tu servidor.
# 3. Levanta todo:
docker compose -f docker-compose.tls.yml up -d
```

En ambos casos:
- `https://chat.midominio.com/`      → cliente web (TLS).
- `https://chat.midominio.com/admin` → panel de administración (TLS).
- `<tu-ip>:5009`                      → clientes Ares (directo, sin TLS).

Ver [`Caddyfile`](./Caddyfile) y [`docker-compose.tls.yml`](./docker-compose.tls.yml).

---

## Configuración

Todos los campos están documentados en [`astra.toml.example`](./astra.toml.example):
puerto, nombre de sala, `owner_password`, capas de seguridad anti-DDoS, captcha,
trusted leaves del Link, rutas de bases GeoIP, etc.

Flags de CLI:

```bash
astra --config astra.toml       # archivo de config
astra --port 6000               # puerto (sobreescribe el toml)
astra --data-dir ./data         # DB, logs, seed, bases GeoIP
astra --link-server             # modo hub
astra --link-client <ip:port>   # modo leaf
astra --no-web / --no-roomsearch
astra seed-refresh              # re-descarga la lista de salas
```

---

## Tests

```bash
cargo test              # toda la suite
cargo test -p proto-ares  # solo el protocolo
cargo bench -p proto-ares # benchmarks
```

---

## Estructura

```
astra/
├── crates/
│   ├── proto-ares/      # Protocolo binario (PacketReader/Writer, TcpMsg, UdpMsg)
│   ├── server-core/     # UserPool, Settings, BanSystem, Captcha, managers, GeoIP
│   ├── udp/             # Room search UDP
│   ├── captcha/         # Generación de captchas
│   ├── commands/        # ~125 comandos slash
│   ├── scripting/       # Motor JS (boa_engine)
│   ├── web/             # WebSockets + panel de administración
│   ├── link/            # Link Hub/Leaf con cifrado AES-256
│   └── astra/           # Binario principal (CLI)
├── docs/                # ARCHITECTURE, PROTOCOL, SECURITY, ROADMAP-V2
├── astra.toml.example
├── docker-compose.yml       # despliegue simple
├── docker-compose.tls.yml   # despliegue con HTTPS (Caddy)
└── Caddyfile
```

## Diferencias con el sb0t original

| Aspecto | sb0t (C#) | Astra (Rust) |
|---|---|---|
| Plataforma | Solo Windows | Linux, Windows, macOS, ARM |
| Runtime | .NET Framework 4.7.2 | Ninguno (binario estático) |
| Concurrencia | Thread + `Thread.Sleep(25ms)` | tokio async |
| Memoria | GC + posibles fugas | Memory-safe por construcción |
| Motor JS | Jurassic (vendoreado) | boa_engine (puro Rust) |
| Administración | GUI WPF (Windows) | Panel web multiplataforma (`/admin`) |
| Link | AES + trusted leaves | AES-256 + trusted leaves |
| Build | msbuild + Visual Studio | `cargo` |

## Licencia

AGPL-3.0-or-later (igual que el sb0t original).
