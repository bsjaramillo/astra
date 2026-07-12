#!/usr/bin/env python3
"""
Prueba de las capas anti-flood / anti-DDoS de Astra.

⚠️  USO AUTORIZADO SOLAMENTE: corré esto contra TU PROPIO servidor Astra (o una
    instancia de prueba). Es una herramienta defensiva para verificar que las
    protecciones se disparan; genera tráfico de ataque simulado.

Capas que verifica (crates/server-core/src/security.rs):

  CAPA 1  Connection flood   — máx. N conexiones nuevas por IP en una ventana;
                               tras varias violaciones, auto-ban temporal.
  CAPA 2  Concurrent limit   — máx. conexiones TCP simultáneas por IP.
  CAPA 3  Handshake timeout  — cierra la conexión si no llega el login a tiempo.
  CAPA 5  Failed-login ban   — auto-ban tras N logins/handshakes fallidos.
  Join-flood                 — rechaza logins válidos demasiado seguidos (misma IP).

IMPORTANTE — el estado de seguridad vive EN MEMORIA (no en disco). Las capas 1 y 5
banean la IP de origen (5 min y 1 hora por defecto). Para un run limpio:
  • Reiniciá el servidor Astra entre tests destructivos (resetea el estado), o
  • Corré un test a la vez con --test <nombre>.
El modo `all` corre primero los tests no-baneantes y deja los baneantes al final.

Ejemplos:
  python3 tools/ddos_test.py --host 127.0.0.1 --port 5009
  python3 tools/ddos_test.py --port 5009 --test concurrent
  python3 tools/ddos_test.py --port 5009 --test flood
"""

import argparse
import socket
import struct
import sys
import time

# ─────────────────────────── framing Ares ───────────────────────────
# Paquete cliente→server: [size:u16 LE][op:u8][payload], size = len(payload).

def ares_packet(opcode: int, payload: bytes = b"") -> bytes:
    body = bytes([opcode]) + payload
    return struct.pack("<H", len(body) - 1) + body

def cstr(s: str) -> bytes:
    return s.encode("utf-8") + b"\x00"

TCP_CLIENT_LOGIN = 2  # proto-ares TcpMsg::ClientLogin

def build_login(name: str, guid16: bytes, version: str = "Ares 2.1.0") -> bytes:
    """Login TCP nativo válido (layout de server-core/src/login.rs::parse_login)."""
    p = b""
    p += guid16                       # 16 bytes GUID
    p += struct.pack("<H", 0)         # file_count
    p += bytes([0])                   # crypto byte (0 = sin cifrado)
    p += struct.pack("<H", 1234)      # data_port
    p += bytes([1, 2, 3, 4])          # node_ip
    p += struct.pack("<H", 5009)      # node_port
    p += bytes([0, 0, 0, 0])          # skip4
    p += cstr(name)                   # org_name
    p += cstr(version)                # version
    p += bytes([192, 168, 1, 100])    # local_ip
    p += bytes([0, 0, 0, 0])          # skip4b
    p += bytes([0])                   # browsable
    p += bytes([0, 0, 0])             # current/max uploads, queued
    p += bytes([25])                  # age
    p += bytes([1])                   # sex
    p += bytes([49])                  # country
    p += cstr("US")                   # region
    return ares_packet(TCP_CLIENT_LOGIN, p)


# ─────────────────────────── helpers de red ───────────────────────────

def connect(host, port, timeout=4.0, probe=False):
    """Abre una conexión TCP. Con `probe=True` manda 1 byte para que el
    servidor la clasifique como cliente Ares nativo y aplique las capas de
    seguridad (CAPA 1/2/5 corren DESPUÉS de la clasificación, que necesita el
    primer byte; una conexión totalmente muda se queda en el `peek()` de
    demux y NO pasa por esas capas — ver nota Slowloris en el README)."""
    s = socket.create_connection((host, port), timeout=timeout)
    if probe:
        try:
            s.sendall(b"\x02")  # opcode ClientLogin (incompleto): clasifica como Ares
        except OSError:
            pass
    return s

def read_reply(sock, wait=0.6) -> str:
    """Lee lo que mande el server durante `wait` seg. Devuelve el texto legible
    (los mensajes de rechazo son ASCII). Vacío = el server no rechazó (aceptó y
    está esperando el login)."""
    sock.settimeout(wait)
    data = b""
    try:
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
            if len(data) > 4096:
                break
    except socket.timeout:
        pass
    except OSError:
        pass
    # extraer los bytes imprimibles (el mensaje null-terminated)
    return "".join(chr(b) if 32 <= b < 127 else " " for b in data).strip()

# frases de rechazo esperadas por capa (RejectReason::message()).
MSG = {
    "flood": "Too many connections",
    "flood_ban": "temporarily banned for connection flooding",
    "concurrent": "Too many simultaneous connections",
    "failed": "Too many failed login attempts",
    "joinflood": "Joining too quickly",
    "banned": "banned",
}

class Result:
    def __init__(self, name):
        self.name = name
        self.ok = None      # True / False / None (inconcluso)
        self.detail = ""

def hdr(t):
    print("\n" + "═" * 68)
    print(f"  {t}")
    print("═" * 68)


# ─────────────────────────── tests por capa ───────────────────────────

def test_concurrent(host, port, cfg) -> Result:
    """CAPA 2: abrir más conexiones simultáneas que el límite y NO cerrarlas.
    Las que exceden `max_concurrent_per_ip` deben rechazarse."""
    r = Result("CAPA 2 · Concurrentes por IP")
    hdr(r.name + f"  (límite esperado: {cfg.concurrent})")
    n = cfg.concurrent + 3
    socks, accepted, rejected = [], 0, 0
    for i in range(n):
        try:
            s = connect(host, port, probe=True)
        except OSError as e:
            print(f"  conex {i+1}: no se pudo conectar ({e})")
            continue
        socks.append(s)
        reply = read_reply(s, 0.4)
        if MSG["concurrent"] in reply:
            rejected += 1
            print(f"  conex {i+1}: RECHAZADA → «{reply[:60]}»")
        elif reply == "":
            accepted += 1
            print(f"  conex {i+1}: aceptada (esperando login)")
        else:
            print(f"  conex {i+1}: otra respuesta → «{reply[:60]}»")
    for s in socks:
        s.close()
    r.ok = rejected > 0
    r.detail = f"{accepted} aceptadas, {rejected} rechazadas por límite concurrente"
    print(f"\n  → {'✅ PROTECCIÓN ACTIVA' if r.ok else '❌ NO se rechazó ninguna'}: {r.detail}")
    return r


def test_handshake(host, port, cfg) -> Result:
    """CAPA 3: conectar y NO mandar el login. El server debe cerrar la conexión
    alrededor de `handshake_timeout_secs`."""
    r = Result("CAPA 3 · Handshake timeout")
    hdr(r.name + f"  (timeout esperado: ~{cfg.handshake}s)")
    try:
        # probe=True: manda 1 byte para clasificar como Ares nativo. CAPA 3
        # corre dentro del handler TCP (post-clasificación); una conexión SIN
        # ningún byte se queda en el peek de demux (ver test 'slowloris').
        s = connect(host, port, probe=True)
    except OSError as e:
        r.ok = None; r.detail = f"no conecta: {e}"; print("  " + r.detail); return r
    t0 = time.time()
    s.settimeout(cfg.handshake + 8)
    closed_at = None
    try:
        while True:
            chunk = s.recv(4096)
            if not chunk:
                closed_at = time.time() - t0
                break
    except socket.timeout:
        pass
    except OSError:
        closed_at = time.time() - t0
    s.close()
    if closed_at is not None:
        r.ok = closed_at <= cfg.handshake + 6
        r.detail = f"el server cerró la conexión a los {closed_at:.1f}s"
    else:
        r.ok = False
        r.detail = f"la conexión seguía abierta tras {cfg.handshake + 8}s (no se cerró)"
    print(f"\n  → {'✅ PROTECCIÓN ACTIVA' if r.ok else '❌ FALLA'}: {r.detail}")
    return r


def test_joinflood(host, port, cfg) -> Result:
    """Join-flood: un login válido y, de inmediato, otro con nick distinto desde
    la misma IP. El segundo debe rechazarse por 'Joining too quickly'."""
    r = Result("Join-flood (logins válidos demasiado seguidos)")
    hdr(r.name)
    guid = b"\xAA" * 16
    # 1er login (debería entrar)
    try:
        s1 = connect(host, port)
        s1.sendall(build_login("FloodA", guid))
        rep1 = read_reply(s1, 0.8)
        print(f"  login 1 (FloodA): {'aceptado' if 'Joining' not in rep1 else 'rechazado'} «{rep1[:50]}»")
    except OSError as e:
        r.ok = None; r.detail = f"no conecta: {e}"; print("  " + r.detail); return r
    # 2do login inmediato, nick distinto, misma IP
    try:
        s2 = connect(host, port)
        s2.sendall(build_login("FloodB", b"\xBB" * 16))
        rep2 = read_reply(s2, 0.8)
    except OSError as e:
        rep2 = f"(error: {e})"
    r.ok = MSG["joinflood"] in rep2
    r.detail = f"2do login inmediato → «{rep2[:60]}»"
    for s in (locals().get("s1"), locals().get("s2")):
        try: s.close()
        except Exception: pass
    print(f"\n  → {'✅ PROTECCIÓN ACTIVA' if r.ok else '⚠️  no se disparó'}: {r.detail}")
    return r


def test_flood(host, port, cfg) -> Result:
    """CAPA 1: abrir+cerrar muchas conexiones rápido. Tras el límite deben
    aparecer rechazos por flood y, luego, el auto-ban temporal.
    ⚠️ BANEA la IP de origen por `connection_flood_ban_secs` (default 5 min)."""
    r = Result("CAPA 1 · Connection flood + auto-ban")
    hdr(r.name + f"  (límite: {cfg.newconn}/{cfg.window}s · BANEA la IP)")
    n = cfg.newconn + 8
    flood = ban = accepted = 0
    for i in range(n):
        try:
            s = connect(host, port, probe=True)
        except OSError as e:
            print(f"  conex {i+1}: no conecta ({e})")
            continue
        reply = read_reply(s, 0.3)
        s.close()  # cerrar para no chocar con CAPA 2
        if MSG["flood_ban"] in reply:
            ban += 1; tag = "AUTO-BAN"
        elif MSG["flood"] in reply:
            flood += 1; tag = "flood"
        elif reply == "":
            accepted += 1; tag = "aceptada"
        else:
            tag = f"otra: «{reply[:40]}»"
        print(f"  conex {i+1:2}: {tag}")
    r.ok = flood > 0 or ban > 0
    r.detail = f"{accepted} aceptadas, {flood} rechazos flood, {ban} auto-ban"
    print(f"\n  → {'✅ PROTECCIÓN ACTIVA' if r.ok else '❌ NO se activó el flood'}: {r.detail}")
    if ban:
        print("  ⚠️  La IP quedó BANEADA temporalmente. Reiniciá el server para resetear.")
    return r


def test_failed(host, port, cfg) -> Result:
    """CAPA 5: mandar varios logins/paquetes inválidos. Tras `max_failed_logins`
    la IP debe quedar baneada ('Too many failed login attempts').
    ⚠️ BANEA la IP por `failed_login_ban_secs` (default 1 HORA)."""
    r = Result("CAPA 5 · Ban por logins fallidos")
    hdr(r.name + f"  (límite: {cfg.failed} fallos · BANEA la IP 1h)")
    n = cfg.failed + 3
    unknown = banned = 0
    for i in range(n):
        try:
            s = connect(host, port)
        except OSError as e:
            print(f"  intento {i+1}: no conecta ({e})")
            continue
        s.sendall(ares_packet(200))  # opcode desconocido → login fallido
        reply = read_reply(s, 0.4)
        s.close()
        if MSG["failed"] in reply:
            banned += 1; tag = "BANEADO (capa 5)"
        elif "Unknown protocol" in reply or "login" in reply.lower():
            unknown += 1; tag = f"fallo registrado «{reply[:40]}»"
        else:
            tag = f"otra: «{reply[:40]}»"
        print(f"  intento {i+1:2}: {tag}")
    r.ok = banned > 0
    r.detail = f"{unknown} fallos registrados, {banned} rechazos por ban"
    print(f"\n  → {'✅ PROTECCIÓN ACTIVA' if r.ok else '❌ NO se activó el ban'}: {r.detail}")
    if banned:
        print("  ⚠️  La IP quedó BANEADA (1h por defecto). Reiniciá el server para resetear.")
    return r


def test_slowloris(host, port, cfg) -> Result:
    """Slowloris: conexiones que se abren y NO mandan ni un byte. Con el fix,
    la clasificación (peek) tiene un timeout = handshake_timeout, así que el
    server las cierra en vez de dejarlas colgadas para siempre. Este test abre
    muchas conexiones mudas y verifica que el server las cierre tras el timeout.

    (El cap de conexiones crudas por IP —el otro fix— exime loopback/proxies,
    así que desde localhost no se observa; corré desde una IP remota para verlo.
    Acá se valida el timeout, que sí aplica a todas.)"""
    r = Result("Slowloris · conexiones mudas (sin byte)")
    wait = cfg.handshake + 4
    hdr(r.name + f"  (deben cerrarse a ~{cfg.handshake}s por el timeout de clasificación)")
    n = cfg.concurrent + 15
    socks = []
    for _ in range(n):
        try:
            socks.append(connect(host, port))  # SIN probe: no manda nada
        except OSError:
            break
    print(f"  abiertas {len(socks)} conexiones mudas")
    print(f"  esperando {wait}s (handshake_timeout={cfg.handshake}s) a que el server las cierre…")
    time.sleep(wait)
    alive = 0
    for s in socks:
        s.settimeout(0.3)
        try:
            if s.recv(1) != b"":   # llegó algo pero no EOF → sigue viva
                alive += 1
        except socket.timeout:
            alive += 1             # sigue abierta, sin datos
        except OSError:
            pass
        s.close()
    r.ok = alive == 0
    r.detail = f"{alive}/{len(socks)} conexiones mudas seguían vivas tras {wait}s"
    if r.ok:
        print(f"\n  → ✅ PROTEGIDO: el server cerró todas las conexiones mudas")
    else:
        print(f"\n  → ⚠️  HUECO (Slowloris): {r.detail}")
        print("     Las conexiones mudas no se cierran; un atacante puede acumularlas.")
    return r


TESTS = {
    "concurrent": test_concurrent,
    "handshake": test_handshake,
    "joinflood": test_joinflood,
    "slowloris": test_slowloris,
    "flood": test_flood,
    "failed": test_failed,
}
# Orden en modo `all`: primero los no-baneantes; los baneantes al final.
ALL_ORDER = ["concurrent", "joinflood", "slowloris", "handshake", "flood", "failed"]


def main():
    ap = argparse.ArgumentParser(description="Prueba de las capas anti-DDoS de Astra (uso autorizado).")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5009)
    ap.add_argument("--test", choices=["all"] + list(TESTS), default="all")
    # overrides de los defaults de SecurityConfig (por si cambiaste el config)
    ap.add_argument("--newconn", type=int, default=10, help="max_new_connections_per_ip")
    ap.add_argument("--window", type=int, default=60, help="connection_window_secs")
    ap.add_argument("--concurrent", type=int, default=5, help="max_concurrent_per_ip")
    ap.add_argument("--handshake", type=int, default=15, help="handshake_timeout_secs")
    ap.add_argument("--failed", type=int, default=5, help="max_failed_logins")
    ap.add_argument("--yes", action="store_true", help="no pedir confirmación para los tests que banean")
    cfg = ap.parse_args()

    print(f"Objetivo: {cfg.host}:{cfg.port}")
    try:
        connect(cfg.host, cfg.port).close()
    except OSError as e:
        print(f"❌ No se puede conectar a {cfg.host}:{cfg.port}: {e}")
        sys.exit(2)

    order = ALL_ORDER if cfg.test == "all" else [cfg.test]
    destructive = {"flood", "failed"}
    if any(t in destructive for t in order) and not cfg.yes:
        print("\n⚠️  Los tests 'flood' y 'failed' BANEAN la IP de origen (en memoria).")
        print("   Reiniciá el server Astra para resetear el estado entre corridas.")
        try:
            if input("   ¿Continuar? [s/N] ").strip().lower() not in ("s", "y", "si", "yes"):
                print("Cancelado."); sys.exit(0)
        except EOFError:
            print("   (sin TTY; usá --yes para confirmar). Cancelado."); sys.exit(0)

    results = []
    for name in order:
        try:
            results.append(TESTS[name](cfg.host, cfg.port, cfg))
        except Exception as e:  # noqa
            r = Result(name); r.ok = None; r.detail = f"error: {e}"
            results.append(r)
        # tras un test que banea, avisar que lo demás puede salir "ya baneado"
        if name in destructive and name != order[-1]:
            print("\n  (nota: la IP puede estar baneada ahora; reiniciá el server "
                  "antes del próximo test destructivo para un resultado limpio.)")

    hdr("RESUMEN")
    for r in results:
        mark = {True: "✅", False: "❌", None: "⚠️ "}[r.ok]
        print(f"  {mark}  {r.name:44} {r.detail}")
    fails = [r for r in results if r.ok is False]
    print()
    if fails:
        print(f"❌ {len(fails)} capa(s) NO se activaron como se esperaba. Revisá la config/logs.")
        sys.exit(1)
    print("✅ Todas las capas probadas se activaron correctamente.")


if __name__ == "__main__":
    main()
