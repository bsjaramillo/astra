//! HTML panel simple para probar el WebSocket.

/// HTML simple con un chat de prueba. Sirve como punto de entrada
/// para clientes HTML5 que quieran probar el WebSocket.
pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="UTF-8">
<title>Astra Chat</title>
<style>
  body { font-family: monospace; margin: 0; padding: 10px; background: #1e1e1e; color: #ddd; }
  #msgs { height: 70vh; overflow-y: auto; border: 1px solid #555; padding: 8px; margin-bottom: 8px; }
  #input { width: 80%; padding: 8px; background: #2a2a2a; color: #ddd; border: 1px solid #555; }
  button { padding: 8px; }
  .msg { margin: 2px 0; }
  .nick { color: #4ec9b0; font-weight: bold; }
  .pm { color: #dcdcaa; font-style: italic; }
  .topic { color: #9cdcfe; }
</style>
</head>
<body>
<h2>Astra WebSocket Test</h2>
<div id="msgs"></div>
<input id="input" type="text" placeholder="Login con LOGIN:1,32,5:...">
<button onclick="send()">Send</button>

<script>
const ws = new WebSocket((location.protocol === "https:" ? "wss://" : "ws://") + location.host + "/");
const msgs = document.getElementById("msgs");
const input = document.getElementById("input");

function log(text, cls) {
  const d = document.createElement("div");
  d.className = "msg" + (cls ? " " + cls : "");
  d.textContent = text;
  msgs.appendChild(d);
  msgs.scrollTop = msgs.scrollHeight;
}

ws.onopen = () => {
  log("[WS] conectado");
  // Auto-login
  const guid = "A".repeat(32);
  const login = "LOGIN:4,32,7,2:2000" + guid + "WebUseres";
  ws.send(login);
  log("[>] " + login);
};

// Parser length-prefixed ib0t: "IDENT:len1,len2,...:val1val2...".
// Devuelve {ident, fields:[...], rest} donde `rest` es lo que queda tras
// consumir los campos por longitud (para el nivel/flags de USERINFO/USERLIST,
// cuyo length-prefix dice "1" aunque el valor sea el byte decimal completo).
function parseLP(data) {
  const i1 = data.indexOf(":");
  if (i1 < 0) return { ident: data, fields: [], rest: "" };
  const ident = data.substring(0, i1);
  const after = data.substring(i1 + 1);
  const i2 = after.indexOf(":");
  if (i2 < 0 || !/^[0-9,]*$/.test(after.substring(0, i2))) {
    return { ident, fields: [], rest: after };
  }
  const lens = after.substring(0, i2).split(",").filter(x => x !== "").map(Number);
  const body = after.substring(i2 + 1);
  const fields = [];
  let pos = 0;
  // Solo consumimos por longitud los campos "reales" (los que no son ",1"
  // sintéticos de nivel/flags). Para simplificar, consumimos todos menos
  // dejamos que el consumidor mire `rest` cuando aplique.
  for (const l of lens) { fields.push(body.substr(pos, l)); pos += l; }
  return { ident, fields, rest: body.substring(pos), body };
}

ws.onmessage = (e) => {
  log("[<] " + e.data);
  const m = parseLP(e.data);
  if (m.ident === "ACK") {
    log("✓ Logueado como " + m.fields[0], "nick");
  } else if (m.ident === "TOPIC" || m.ident === "TOPIC_FIRST") {
    log("📌 " + m.fields[0], "topic");
  } else if (m.ident === "PUBLIC") {
    const from = m.fields[0] || "server";
    log("[" + from + "] " + (m.fields[1] || ""));
  } else if (m.ident === "EMOTE") {
    log("* " + m.fields[0] + " " + (m.fields[1] || ""));
  } else if (m.ident === "PM") {
    log("💬 " + m.fields[0] + ": " + (m.fields[1] || ""), "pm");
  } else if (m.ident === "USERLIST" || m.ident === "USERINFO" || m.ident === "JOININFO") {
    // fields[0] = name; el nivel/flags quedan en `rest` (o al final del body).
    log("👤 " + m.fields[0], "nick");
  } else if (m.ident === "OFFLINE") {
    log("👋 " + m.fields[0] + " salió");
  } else if (m.ident === "NOSUCH") {
    log("⚠ " + m.fields[0]);
  }
};

ws.onerror = (e) => log("[WS] error: " + e);
ws.onclose = () => log("[WS] desconectado");

function send() {
  const text = input.value;
  if (!text) return;
  ws.send("PUBLIC:" + text);
  input.value = "";
}
input.addEventListener("keypress", (e) => { if (e.key === "Enter") send(); });
</script>
</body>
</html>
"#;

/// Panel de administración web (single-page). Servido en `GET /admin`.
/// Auth por owner password → token bearer; todas las acciones se ejecutan
/// vía `POST /admin/cmd` (que corre comandos slash como Owner).
///
/// Rediseño 2026-07: mobile-first (la mayoría de los admins lo usan desde el
/// teléfono), con navegación agrupada en un cajón lateral, lenguaje pensado
/// para usuarios no técnicos, toggles/tarjetas en vez de tablas apretadas y
/// notificaciones tipo toast. Bilingüe español/inglés (diccionario `I18N` +
/// `t()`, detección por `navigator.language`, selector en el header,
/// persistido en `sessionStorage`). El contrato con el backend (endpoints
/// `/admin/*`, campos del STATE, comandos slash) es idéntico — solo cambia la
/// capa de presentación.
pub const ADMIN_HTML: &str = r####"<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>Astra · Administración</title>
<style>
  :root{
    --bg:#0f1116; --surface:#171a21; --surface2:#1f232d; --surface3:#272c38;
    --border:#2b303c; --fg:#e7eaf0; --mut:#9aa2b1; --mut2:#6b7280;
    --acc:#ff7a1a; --acc-d:#e06612; --acc-soft:rgba(255,122,26,.14);
    --ok:#34c759; --danger:#ff5257; --warn:#ffb020;
    --lvl-owner:#ffb454; --lvl-admin:#ff5257; --lvl-mod:#4f8cff; --lvl-voice:#34c759;
    --radius:14px; --radius-sm:10px;
  }
  *{box-sizing:border-box}
  html,body{margin:0}
  body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;background:var(--bg);color:var(--fg);font-size:15px;line-height:1.45;-webkit-text-size-adjust:100%}
  a{color:var(--acc);text-decoration:none}
  h2{font-size:19px;margin:0 0 2px;font-weight:700}
  h3{font-size:15px;margin:0 0 12px;font-weight:600;display:flex;align-items:center;gap:8px}
  .mut{color:var(--mut)}
  .hidden{display:none!important}

  .btn{appearance:none;border:1px solid var(--border);background:var(--surface2);color:var(--fg);border-radius:10px;padding:9px 13px;font-size:14px;font-weight:500;cursor:pointer;display:inline-flex;align-items:center;justify-content:center;gap:6px;min-height:40px;transition:.12s;white-space:nowrap}
  .btn:hover{border-color:var(--acc);color:#fff}
  .btn:active{transform:translateY(1px)}
  .btn.sm{padding:7px 10px;font-size:13px;min-height:34px;border-radius:9px}
  .btn.primary{background:var(--acc);border-color:var(--acc);color:#211100;font-weight:700}
  .btn.primary:hover{background:var(--acc-d);border-color:var(--acc-d);color:#211100}
  .btn.danger{color:#ff9195;border-color:#5a2b2e}
  .btn.danger:hover{background:#3a1e20;border-color:var(--danger);color:#fff}
  .btn.ghost{background:transparent}
  .iconbtn{appearance:none;border:1px solid transparent;background:transparent;color:var(--fg);font-size:18px;width:40px;height:40px;border-radius:10px;cursor:pointer;display:inline-flex;align-items:center;justify-content:center}
  .iconbtn:hover{background:var(--surface2)}

  input,select,textarea{width:100%;background:var(--bg);color:var(--fg);border:1px solid var(--border);border-radius:10px;padding:10px 12px;font-size:14px;font-family:inherit}
  input:focus,select:focus,textarea:focus{outline:none;border-color:var(--acc)}
  select{cursor:pointer}
  .sel.sm{width:auto;min-height:34px;padding:6px 26px 6px 10px;font-size:13px}
  textarea{resize:vertical}
  label.fld{display:block;margin-bottom:13px}
  label.fld>span{display:block;font-size:13px;color:var(--mut);margin-bottom:5px}
  .inline{display:flex;gap:8px}
  .inline>input{flex:1}
  .check{display:flex;align-items:center;gap:10px;padding:12px;background:var(--surface2);border:1px solid var(--border);border-radius:10px;cursor:pointer;font-size:14px;margin-bottom:9px}
  .check input{width:19px;height:19px;accent-color:var(--acc)}

  header{position:sticky;top:0;z-index:40;display:flex;align-items:center;gap:9px;padding:9px 12px;padding-top:calc(9px + env(safe-area-inset-top));background:rgba(23,26,33,.94);backdrop-filter:blur(10px);border-bottom:1px solid var(--border)}
  .brand{display:flex;align-items:center;gap:8px;font-weight:700;font-size:16px}
  .logo{width:28px;height:28px;border-radius:8px;background:linear-gradient(147deg,#ff9a3d,#ff5e00);display:flex;align-items:center;justify-content:center;flex:none}
  .logo svg{width:68%;height:68%;color:#fff;display:block}
  .hstats{color:var(--mut);font-size:12.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
  .spacer{flex:1}
  .only-desktop{display:none}

  .layout{display:block}
  aside{position:fixed;top:0;left:0;bottom:0;width:266px;background:var(--surface);border-right:1px solid var(--border);z-index:60;transform:translateX(-100%);transition:transform .22s ease;overflow-y:auto;padding:14px 10px calc(30px + env(safe-area-inset-bottom))}
  aside.open{transform:none}
  .backdrop{position:fixed;inset:0;background:rgba(0,0,0,.55);z-index:50;opacity:0;pointer-events:none;transition:opacity .2s}
  .backdrop.show{opacity:1;pointer-events:auto}
  .navgroup{margin-bottom:14px}
  .navtitle{font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:var(--mut2);padding:6px 12px 4px;font-weight:700}
  .navitem{display:flex;align-items:center;width:100%;justify-content:flex-start;background:transparent;border:none;color:var(--mut);border-radius:10px;padding:11px 12px;font-size:14.5px;gap:11px;min-height:44px;cursor:pointer;font-family:inherit;text-align:left}
  .navitem:hover{background:var(--surface2);color:var(--fg)}
  .navitem.active{background:var(--acc-soft);color:var(--acc);font-weight:600}
  .ni-ic{font-size:17px;width:22px;text-align:center;flex:none}
  main{padding:16px 14px calc(60px + env(safe-area-inset-bottom));max-width:920px;margin:0 auto}

  @media(min-width:900px){
    header{padding:10px 18px}
    .layout{display:grid;grid-template-columns:266px 1fr}
    aside{position:sticky;top:55px;height:calc(100vh - 55px);transform:none;z-index:1}
    .backdrop{display:none}
    .only-mobile{display:none}
    .only-desktop{display:inline}
    main{padding:24px 28px 70px}
  }

  .cardhead{margin:2px 2px 16px}
  .sub{color:var(--mut);font-size:13.5px;margin:3px 0 0}
  .card{background:var(--surface);border:1px solid var(--border);border-radius:var(--radius);padding:16px;margin-bottom:16px}
  .tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:12px;margin-bottom:18px}
  .tile{background:var(--surface);border:1px solid var(--border);border-radius:var(--radius-sm);padding:14px 15px}
  .tl{display:block;color:var(--mut);font-size:12.5px;margin-bottom:6px}
  .tv{display:block;font-size:22px;font-weight:700;word-break:break-word}

  .ucards{display:grid;grid-template-columns:repeat(auto-fill,minmax(285px,1fr));gap:12px}
  .ucard{background:var(--surface);border:1px solid var(--border);border-radius:var(--radius);padding:13px 14px}
  .uhead{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:6px}
  .uname{font-size:15.5px;word-break:break-word}
  .umeta{color:var(--mut);font-size:12.5px;margin-bottom:11px;word-break:break-all}
  .uactions{display:flex;flex-wrap:wrap;gap:7px}

  .badge{font-size:11px;font-weight:700;padding:3px 9px;border-radius:20px;background:var(--surface3);color:var(--mut);white-space:nowrap}
  .badge.owner{background:rgba(255,180,84,.16);color:var(--lvl-owner)}
  .badge.admin{background:rgba(255,82,87,.16);color:var(--lvl-admin)}
  .badge.mod{background:rgba(79,140,255,.16);color:var(--lvl-mod)}
  .badge.voice{background:rgba(52,199,89,.16);color:var(--lvl-voice)}
  .chip{font-size:11.5px;padding:3px 9px;border-radius:20px;background:var(--surface3);color:var(--mut)}
  .chip.warn{background:rgba(255,176,32,.15);color:var(--warn)}
  .pill{display:inline-flex;align-items:center;gap:8px;font-size:13px;padding:7px 12px;border-radius:20px;background:var(--surface2);border:1px solid var(--border);margin:0 7px 7px 0}
  .pill a{color:var(--mut);font-weight:700;font-size:15px;line-height:1}
  .pill a:hover{color:var(--danger)}
  .empty{color:var(--mut);text-align:center;padding:26px 16px;background:var(--surface);border:1px dashed var(--border);border-radius:var(--radius)}

  .tbl{width:100%;border-collapse:collapse}
  .tbl th,.tbl td{text-align:left;padding:10px;border-bottom:1px solid var(--border);font-size:13.5px;vertical-align:middle}
  .tbl th{color:var(--mut);font-weight:700;font-size:11.5px;text-transform:uppercase;letter-spacing:.03em}
  .tbl tr:last-child td{border-bottom:none}
  .scroll{overflow-x:auto;-webkit-overflow-scrolling:touch}

  .rowend{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:14px}
  .grid2{display:grid;grid-template-columns:repeat(auto-fill,minmax(210px,1fr));gap:0 12px}
  code{background:var(--surface3);padding:1px 6px;border-radius:6px;font-size:12.5px}
  .note{background:var(--acc-soft);border:1px solid rgba(255,122,26,.25);border-radius:10px;padding:11px 13px;font-size:13px;color:#ffd0a8;margin-bottom:14px}
  .warnbox{background:rgba(255,176,32,.1);border:1px solid rgba(255,176,32,.3);border-radius:10px;padding:11px 13px;font-size:13px;color:#ffdca0;margin-bottom:14px}

  #console-out{background:#0a0c10;border:1px solid var(--border);border-radius:12px;padding:12px;height:340px;overflow-y:auto;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12.5px;white-space:pre-wrap;line-height:1.5}

  .flags{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:11px}
  .flag{display:flex;align-items:center;justify-content:space-between;gap:12px;background:var(--surface);border:1px solid var(--border);border-radius:12px;padding:12px 14px}
  .flag .fn{font-size:14px;font-weight:600}
  .flag .fd{font-size:12px;color:var(--mut);margin-top:2px}
  .switch{position:relative;width:46px;height:26px;flex:none}
  .switch input{opacity:0;width:0;height:0}
  .slider{position:absolute;inset:0;background:var(--surface3);border-radius:20px;transition:.18s;cursor:pointer}
  .slider:before{content:"";position:absolute;width:20px;height:20px;left:3px;top:3px;background:#fff;border-radius:50%;transition:.18s}
  .switch input:checked+.slider{background:var(--acc)}
  .switch input:checked+.slider:before{transform:translateX(20px)}

  .avbox{display:flex;gap:16px;align-items:flex-start;flex-wrap:wrap}
  .avimg{width:96px;height:96px;object-fit:cover;border:1px solid var(--border);border-radius:14px;background:#000;flex:none}
  .avside{flex:1;min-width:200px}

  #login{max-width:380px;margin:14vh auto;padding:0 16px}
  .logincard{background:var(--surface);border:1px solid var(--border);border-radius:18px;padding:28px 22px;text-align:center}
  .logincard .logo{width:54px;height:54px;border-radius:15px;font-size:27px;margin:0 auto 15px}
  .logincard h2{margin-bottom:4px}
  .logincard .sub{margin:0 0 16px}

  #toasts{position:fixed;left:0;right:0;bottom:calc(18px + env(safe-area-inset-bottom));z-index:200;display:flex;flex-direction:column;align-items:center;gap:8px;pointer-events:none;padding:0 12px}
  .toast{background:var(--surface3);border:1px solid var(--border);color:var(--fg);padding:11px 16px;border-radius:12px;font-size:13.5px;max-width:460px;box-shadow:0 8px 26px rgba(0,0,0,.45);opacity:0;transform:translateY(10px);transition:.25s;pointer-events:auto}
  .toast.show{opacity:1;transform:none}
  .toast.ok{border-color:rgba(52,199,89,.5)}
  .toast.err{border-color:rgba(255,82,87,.5)}
</style>
</head>
<body>

<svg width="0" height="0" style="position:absolute" aria-hidden="true"><symbol id="astramark" viewBox="0 0 512 512">
  <g stroke="currentColor" stroke-width="44" stroke-linecap="round" stroke-linejoin="round" fill="none">
    <path d="M164 392 L256 200 L348 392"/><path d="M198 330 L314 330" stroke-width="40"/></g>
  <path d="M256 114 Q260.7 135.3 282 140 Q260.7 144.7 256 166 Q251.3 144.7 230 140 Q251.3 135.3 256 114 Z" fill="currentColor"/>
</symbol></svg>

<div id="login">
  <div class="logincard">
    <div class="logo"><svg viewBox="0 0 512 512"><use href="#astramark"/></svg></div>
    <h2 id="loginTitle">Panel de Astra</h2>
    <p class="sub" id="loginSub">Ingresa la contraseña de dueño para administrar tu sala.</p>
    <input id="pw" type="password" placeholder="Contraseña de dueño" autofocus>
    <div class="rowend" style="justify-content:center">
      <button class="btn primary" id="loginBtn" style="width:100%">Entrar</button>
    </div>
    <div id="loginErr" class="sub" style="color:var(--danger)"></div>
    <div style="margin-top:14px"><a href="#" id="langLink" class="sub">English</a></div>
  </div>
</div>

<div id="app" class="hidden">
<header>
  <button id="menuBtn" class="iconbtn only-mobile" aria-label="Menu">☰</button>
  <div class="brand"><span class="logo"><svg viewBox="0 0 512 512"><use href="#astramark"/></svg></span><span class="only-desktop">Astra</span></div>
  <div class="hstats" id="hdrStat">…</div>
  <div class="spacer"></div>
  <button id="langBtn" class="btn ghost sm" title="Idioma / Language">ES</button>
  <button id="refreshBtn" class="iconbtn" title="Refresh" aria-label="Refresh">↻</button>
  <button id="logoutBtn" class="btn ghost sm">Salir</button>
</header>
<div class="layout">
  <aside id="side"><nav id="nav"></nav></aside>
  <div id="backdrop" class="backdrop"></div>
  <main id="view"></main>
</div>
</div>

<div id="toasts"></div>

<script>
let TOKEN = null;
let STATE = {};
let TAB = "inicio";
let CONFIG = null;
let LANG = "es";

/* ============================ i18n ============================ */
const I18N = {
  es:{
    chrome_refresh:"Actualizar", chrome_logout:"Salir", chrome_menu:"Menú",
    login_title:"Panel de Astra", login_sub:"Ingresa la contraseña de dueño para administrar tu sala.",
    login_pw:"Contraseña de dueño", login_btn:"Entrar", login_err:"Contraseña incorrecta.", login_switch:"English",
    hdr_online:"en línea", hdr_peak:"pico", hdr_bans:"baneos",
    g_principal:"Principal", g_moderacion:"Moderación", g_sala:"Sala", g_avanzado:"Avanzado",
    nav_inicio:"Inicio", nav_usuarios:"Usuarios", nav_cuentas:"Cuentas", nav_baneos:"Baneos",
    nav_filtros:"Filtros de palabras", nav_bienvenidas:"Bienvenidas", nav_sala:"Opciones de sala",
    nav_avatares:"Avatares", nav_servidor:"Servidor", nav_enlace:"Enlace de salas", nav_seguridad:"Seguridad",
    nav_proxies:"Proxies", nav_permisos:"Permisos de comandos", nav_config:"Config avanzada", nav_consola:"Consola",
    nav_motd:"Mensaje de entrada", nav_plantillas:"Textos del sistema",
    common_save:"Guardar", common_save_changes:"Guardar cambios", common_add:"Agregar", common_remove:"Quitar",
    common_none:"Ninguno.", common_none_f:"Ninguna.", common_done:"Listo",
    restart_note:"⚠️ Estos cambios se guardan en el archivo de configuración y se aplican al <b>reiniciar el servidor</b>.",
    saved_restart:"Guardado. Reinicia el servidor para aplicar los cambios.",
    err_prefix:"Error: ", err_save:"no se pudo guardar",

    inicio_h:"Inicio", inicio_sub:"Estado general de tu sala, en tiempo real.",
    tile_room:"Sala", tile_bot:"Bot", tile_online:"En línea", tile_peak:"Pico",
    tile_total:"Ingresos totales", tile_bans:"Baneos activos", tile_uptime:"Tiempo activo",
    inicio_topic_h:"💬 Tema y estado", inicio_topic_l:"Tema de la sala (topic)",
    inicio_status_l:"Estado (mensaje corto)", inicio_status_ph:"ej. sala en mantenimiento",
    toast_topic:"Tema actualizado", toast_status:"Estado actualizado",

    users_h:"Usuarios en línea", users_sub:"{0} conectado(s). Toca una acción para moderar.",
    users_empty:"No hay nadie conectado en este momento.",
    u_muted:"silenciado", u_room:"sala", u_files:"archivos",
    u_info:"ℹ️ Info", u_kick:"👢 Expulsar", u_ban:"🚫 Banear", u_mute:"🔇 Silenciar", u_unmute:"🔊 Reactivar",
    u_changerank:"Cambiar rango…", u_to_voice:"→ Voz", u_to_mod:"→ Moderador", u_to_admin:"→ Administrador", u_remrank:"→ Quitar rango",
    cf_ban:"¿Seguro que quieres banear a {0}?",
    toast_kicked:"Expulsado: {0}", toast_banned:"Baneado: {0}", toast_muted:"Silenciado: {0}", toast_unmuted:"Reactivado: {0}",
    toast_rank_rem:"Rango quitado a {0}", toast_rank_upd:"Rango actualizado: {0}",

    accounts_h:"Cuentas registradas", accounts_sub:"{0} cuenta(s) guardada(s) con contraseña.",
    accounts_note:"Para dar o quitar rangos usa la pestaña <b>Usuarios</b> (aplica al instante a quien esté conectado). El rango se recuerda cuando la persona vuelve a entrar con su contraseña.",
    accounts_empty:"No hay cuentas registradas.", th_rank:"Rango", th_name:"Nombre",

    bans_h:"Baneos", bans_sub:"Personas y redes bloqueadas de tu sala.",
    bans_users_h:"🚫 Usuarios baneados", bans_users_empty:"No hay usuarios baneados.",
    bans_clear:"Vaciar todos los baneos", cf_clear:"¿Vaciar TODOS los baneos? No se puede deshacer.",
    toast_ban_rem:"Baneo quitado", toast_cleared:"Baneos vaciados",
    bans_range_h:"📡 Baneos por rango de IP", bans_range_desc:"Bloquea un rango entero de direcciones. Escribe el prefijo, ej. <code>1.2.3.</code>",
    toast_range_ban:"Rango bloqueado", toast_range_unban:"Rango desbloqueado",
    bans_asn_h:"🌍 Baneos por red (ASN)", bans_asn_desc:"Bloquea una red/proveedor completo por su número ASN.",
    bans_asn_ph:"Número de ASN, ej. 12345", asn_pill:"Red AS{0}",
    toast_asn_ban:"Red bloqueada", toast_asn_unban:"Red desbloqueada",

    filters_h:"Filtros de palabras", filters_sub:"Reglas que actúan cuando alguien escribe cierta palabra.",
    filters_note:"<b>¿Qué hace cada acción?</b> · <b>Bloquear</b>: censura el mensaje · <b>Expulsar</b>: echa a quien la use · <b>Banear</b>: la banea · <b>Anunciar</b>: deja pasar el mensaje y manda respuestas automáticas (se editan con la consola: <code>/addline</code>).",
    filters_active_h:"🧹 Filtros activos", filters_empty:"No hay filtros.",
    th_word:"Palabra / patrón", th_action:"Acción",
    filters_ph:"palabra (se admiten * y ?)", filters_add:"Agregar filtro",
    toast_filter_add:"Filtro agregado", toast_filter_rem:"Filtro quitado",

    greets_h:"Mensajes de bienvenida", greets_sub:"Se muestran a quien entra a la sala. Estado actual: ",
    greets_on:"activados", greets_off:"desactivados",
    greets_note:"Puedes usar comodines: <code>+n</code> = nombre de quien entra · <code>+rn</code> = nombre de la sala.",
    th_message:"Mensaje", greets_empty:"No hay mensajes de bienvenida.",
    greets_ph:"¡Bienvenido/a +n a +rn!", greets_disable:"Desactivar todos", greets_enable:"Activar",
    toast_greet_add:"Bienvenida agregada", toast_greet_rem:"Bienvenida quitada", toast_toggled:"Actualizado",

    sala_h:"Opciones de la sala", sala_sub:"Activa o desactiva funciones. Los cambios se aplican al instante.", sala_empty:"Sin opciones.",

    av_h:"Avatares", av_sub:"Imágenes que usa el servidor.",
    av_room_h:"🏠 Avatar de la sala", av_room_desc:"Se envía a cada cliente Ares al entrar y se actualiza en vivo para todos.",
    av_def_h:"👤 Avatar por defecto", av_def_desc:"Se asigna a los clientes Ares que no envían su propio avatar dentro de los primeros 10 segundos.",
    av_upload:"Subir imagen", av_pick:"Elige una imagen primero.", av_updated:"Imagen actualizada.", av_err:"no se pudo subir",

    srv_h:"Servidor", srv_sub:"Datos básicos de tu servidor.",
    srv_roomname:"Nombre de la sala", srv_topic:"Tema por defecto", srv_bot:"Nombre del bot",
    srv_port:"Puerto principal", srv_webport:"Puerto web", srv_ownerpw:"Contraseña de dueño",
    srv_lang:"Idioma (0 = inglés)", srv_datadir:"Carpeta de datos",
    srv_webon:"Web / clientes ib0t habilitados", srv_allowreg:"Permitir registro de cuentas", srv_roomsearch:"Aparecer en la búsqueda de salas (UDP)",
    srv_seedurl:"URL del seed de búsqueda de salas", srv_seedurl_hint:"JSON de rooms para propagarse en la red Ares al arrancar. Vacío = descarga automática desactivada.",
    srv_override_hint:"Si el servidor arrancó con <code>--port</code> o <code>--data-dir</code> (es lo que hace el docker-compose generado por astra-creator), esos argumentos GANAN sobre estos campos y editarlos acá no tiene efecto. Cambia el puerto en el compose (también hay que ajustar el mapeo <code>ports:</code>).",

    link_h:"Enlace de salas", link_sub:"Conecta tu sala con otros servidores (Link Hub).",
    link_warn:"⚠️ Requiere <b>reiniciar el servidor</b>. El Link Hub viaja por el puerto principal (no usa un puerto aparte).",
    link_enable:"Activar Link Hub", link_guid:"GUID del servidor",
    link_leaves_h:"🍃 Salas hijas de confianza",
    link_leaves_desc:"Sin ninguna en la lista: modo legado (se acepta cualquier hija, sin cifrar). Con al menos una, solo se aceptan las que coincidan y la conexión se cifra.",
    th_guid:"GUID", link_leaf_name_ph:"nombre de la sala", link_leaf_guid_ph:"guid",

    sec_h:"Seguridad", sec_sub:"Protecciones anti-flood, anti-bot y captcha.",
    sec_warn:"⚠️ Requiere <b>reiniciar el servidor</b>. Si no sabes qué hace un valor, es mejor dejarlo como está.",
    sec_conn_h:"🚪 Conexiones",
    sec_maxnew:"Máx. conexiones nuevas por IP", sec_window:"Ventana de conteo (seg)",
    sec_floodthr:"Umbral para banear por flood", sec_floodban:"Duración del ban por flood (seg)",
    sec_maxconc:"Máx. conexiones simultáneas por IP", sec_maxraw:"Máx. conexiones crudas por IP (anti-Slowloris, 0=sin límite)", sec_handshake:"Tiempo máx. de login (seg)", sec_idle:"Tiempo máx. inactivo (seg)",
    sec_names_h:"🏷️ Nombres y logins",
    sec_minname:"Largo mínimo de nombre", sec_maxname:"Largo máximo de nombre",
    sec_maxfail:"Máx. logins fallidos", sec_failwin:"Ventana de logins fallidos (seg)", sec_failban:"Ban por logins fallidos (seg)",
    sec_rejectspam:"Rechazar bots de spam automáticamente",
    sec_captcha_h:"🤖 Captcha", sec_captcha_on:"Pedir captcha a las IP nuevas",
    sec_captcha_exp:"Expiración del captcha (seg)", sec_captcha_att:"Intentos permitidos",

    proxy_h:"Proxies de confianza", proxy_sub:"Para cuando tu servidor está detrás de un proxy (Cloudflare, nginx, etc.).",
    proxy_note:"Solo las IP de esta lista pueden decir cuál es la IP real del visitante (vía cabeceras <code>X-Forwarded-For</code>/<code>X-Real-IP</code>). Aplica solo a clientes web. La IP local (127.0.0.1) siempre es de confianza. Los cambios se aplican al instante.",
    toast_proxy_add:"Proxy agregado", toast_proxy_rem:"Proxy quitado",

    perm_h:"Permisos de comandos", perm_sub:"Rango mínimo necesario para usar cada comando. Se aplica al instante.",
    perm_search:"🔎 Buscar comando…", th_command:"Comando", th_minrank:"Rango mínimo",
    perm_change:"Cambiar…", perm_custom:"personalizado", perm_reset:"Restaurar",
    toast_perm_upd:"Permiso actualizado", toast_perm_reset:"Permiso restaurado",

    cfg_h:"Config avanzada", cfg_sub:"Editor del archivo <code>astra.toml</code> en crudo. Solo para usuarios avanzados.",
    cfg_warn:"⚠️ Un error aquí puede impedir que el servidor arranque. Para lo cotidiano (opciones de sala, bienvenidas, baneos) usa las otras pestañas. Requiere <b>reiniciar</b> para aplicar.",
    cfg_reload:"Recargar",

    con_h:"Consola", con_sub:"Ejecuta cualquier comando como Dueño.",
    con_note:"Ejemplos: <code>/ban Pedro</code> · <code>/announce hola a todos</code> · <code>/roomflags</code> · <code>/addline 0, texto</code>",
    con_ph:"/comando argumentos", con_run:"Ejecutar",

    motd_h:"Mensaje de entrada (MOTD)", motd_sub:"Se le muestra a cada persona cuando entra a la sala.",
    motd_note:"Una línea por mensaje. Comodines: <code>+n</code> = nombre de quien entra · <code>+rn</code> = nombre de la sala · <code>+uc</code> = usuarios conectados · <code>+ip</code> = IP. Déjalo vacío para no mostrar nada.",
    motd_ph:"¡Bienvenido/a +n a +rn!\nDisfruta tu estadía :)",
    motd_saved:"MOTD guardado.",

    tpl_h:"Textos del sistema", tpl_sub:"Personaliza (o traduce) los mensajes de moderación que ve la gente.",
    tpl_note:"Edita el texto después del <code>=</code> en cada línea (formato <code>clave = texto</code>). Comodines: <code>+n</code> = usuario · <code>+a</code> = admin · <code>+l</code> = nivel · <code>+i</code> = ident. Para restaurar un texto, déjalo igual al original.",
    tpl_warn:"Están cargados todos los mensajes que el servidor le muestra a la gente por los comandos. Los que tienen comodines (como <code>+n</code>) insertan valores al vivo — mantén el comodín si quieres que aparezca ese dato.",
    tpl_saved:"Textos guardados ({0} aplicados).",
  },
  en:{
    chrome_refresh:"Refresh", chrome_logout:"Log out", chrome_menu:"Menu",
    login_title:"Astra Panel", login_sub:"Enter the owner password to manage your room.",
    login_pw:"Owner password", login_btn:"Log in", login_err:"Wrong password.", login_switch:"Español",
    hdr_online:"online", hdr_peak:"peak", hdr_bans:"bans",
    g_principal:"Main", g_moderacion:"Moderation", g_sala:"Room", g_avanzado:"Advanced",
    nav_inicio:"Home", nav_usuarios:"Users", nav_cuentas:"Accounts", nav_baneos:"Bans",
    nav_filtros:"Word filters", nav_bienvenidas:"Greetings", nav_sala:"Room options",
    nav_avatares:"Avatars", nav_servidor:"Server", nav_enlace:"Room linking", nav_seguridad:"Security",
    nav_proxies:"Proxies", nav_permisos:"Command permissions", nav_config:"Advanced config", nav_consola:"Console",
    nav_motd:"Join message", nav_plantillas:"System texts",
    common_save:"Save", common_save_changes:"Save changes", common_add:"Add", common_remove:"Remove",
    common_none:"None.", common_none_f:"None.", common_done:"Done",
    restart_note:"⚠️ These changes are written to the config file and take effect after <b>restarting the server</b>.",
    saved_restart:"Saved. Restart the server to apply the changes.",
    err_prefix:"Error: ", err_save:"couldn't save",

    inicio_h:"Home", inicio_sub:"An overview of your room, in real time.",
    tile_room:"Room", tile_bot:"Bot", tile_online:"Online", tile_peak:"Peak",
    tile_total:"Total joins", tile_bans:"Active bans", tile_uptime:"Uptime",
    inicio_topic_h:"💬 Topic & status", inicio_topic_l:"Room topic",
    inicio_status_l:"Status (short message)", inicio_status_ph:"e.g. room under maintenance",
    toast_topic:"Topic updated", toast_status:"Status updated",

    users_h:"Users online", users_sub:"{0} connected. Tap an action to moderate.",
    users_empty:"Nobody is connected right now.",
    u_muted:"muted", u_room:"room", u_files:"files",
    u_info:"ℹ️ Info", u_kick:"👢 Kick", u_ban:"🚫 Ban", u_mute:"🔇 Mute", u_unmute:"🔊 Unmute",
    u_changerank:"Change rank…", u_to_voice:"→ Voice", u_to_mod:"→ Moderator", u_to_admin:"→ Administrator", u_remrank:"→ Remove rank",
    cf_ban:"Ban {0}?",
    toast_kicked:"Kicked: {0}", toast_banned:"Banned: {0}", toast_muted:"Muted: {0}", toast_unmuted:"Unmuted: {0}",
    toast_rank_rem:"Rank removed from {0}", toast_rank_upd:"Rank updated: {0}",

    accounts_h:"Registered accounts", accounts_sub:"{0} account(s) saved with a password.",
    accounts_note:"To grant or remove ranks use the <b>Users</b> tab (applies instantly to whoever is connected). The rank is remembered when the person logs back in with their password.",
    accounts_empty:"No registered accounts.", th_rank:"Rank", th_name:"Name",

    bans_h:"Bans", bans_sub:"People and networks blocked from your room.",
    bans_users_h:"🚫 Banned users", bans_users_empty:"No banned users.",
    bans_clear:"Clear all bans", cf_clear:"Clear ALL bans? This can't be undone.",
    toast_ban_rem:"Ban removed", toast_cleared:"Bans cleared",
    bans_range_h:"📡 IP range bans", bans_range_desc:"Blocks a whole range of addresses. Type the prefix, e.g. <code>1.2.3.</code>",
    toast_range_ban:"Range blocked", toast_range_unban:"Range unblocked",
    bans_asn_h:"🌍 Network (ASN) bans", bans_asn_desc:"Blocks a whole network/provider by its ASN number.",
    bans_asn_ph:"ASN number, e.g. 12345", asn_pill:"Net AS{0}",
    toast_asn_ban:"Network blocked", toast_asn_unban:"Network unblocked",

    filters_h:"Word filters", filters_sub:"Rules that trigger when someone types a certain word.",
    filters_note:"<b>What does each action do?</b> · <b>Block</b>: censors the message · <b>Kick</b>: kicks whoever uses it · <b>Ban</b>: bans them · <b>Announce</b>: lets the message through and sends automatic replies (edit them from the console: <code>/addline</code>).",
    filters_active_h:"🧹 Active filters", filters_empty:"No filters.",
    th_word:"Word / pattern", th_action:"Action",
    filters_ph:"word (* and ? allowed)", filters_add:"Add filter",
    toast_filter_add:"Filter added", toast_filter_rem:"Filter removed",

    greets_h:"Greeting messages", greets_sub:"Shown to anyone joining the room. Current status: ",
    greets_on:"enabled", greets_off:"disabled",
    greets_note:"You can use placeholders: <code>+n</code> = joining user's name · <code>+rn</code> = room name.",
    th_message:"Message", greets_empty:"No greeting messages.",
    greets_ph:"Welcome +n to +rn!", greets_disable:"Disable all", greets_enable:"Enable",
    toast_greet_add:"Greeting added", toast_greet_rem:"Greeting removed", toast_toggled:"Updated",

    sala_h:"Room options", sala_sub:"Turn features on or off. Changes apply instantly.", sala_empty:"No options.",

    av_h:"Avatars", av_sub:"Images the server uses.",
    av_room_h:"🏠 Room avatar", av_room_desc:"Sent to every Ares client on join and updated live for everyone.",
    av_def_h:"👤 Default avatar", av_def_desc:"Assigned to Ares clients that don't send their own avatar within the first 10 seconds.",
    av_upload:"Upload image", av_pick:"Pick an image first.", av_updated:"Image updated.", av_err:"couldn't upload",

    srv_h:"Server", srv_sub:"Your server's basic settings.",
    srv_roomname:"Room name", srv_topic:"Default topic", srv_bot:"Bot name",
    srv_port:"Main port", srv_webport:"Web port", srv_ownerpw:"Owner password",
    srv_lang:"Language (0 = English)", srv_datadir:"Data folder",
    srv_webon:"Web / ib0t clients enabled", srv_allowreg:"Allow account registration", srv_roomsearch:"Show in room search (UDP)",
    srv_override_hint:"If the server was started with <code>--port</code> or <code>--data-dir</code> (which is what the docker-compose generated by astra-creator does), those arguments WIN over these fields and editing them here has no effect. Change the port in the compose file instead (the <code>ports:</code> mapping needs updating too).",
    srv_seedurl:"Room-search seed URL", srv_seedurl_hint:"rooms JSON used to join the Ares network on startup. Empty = automatic download disabled.",

    link_h:"Room linking", link_sub:"Connect your room with other servers (Link Hub).",
    link_warn:"⚠️ Requires <b>restarting the server</b>. The Link Hub travels over the main port (no separate port).",
    link_enable:"Enable Link Hub", link_guid:"Server GUID",
    link_leaves_h:"🍃 Trusted leaf rooms",
    link_leaves_desc:"None in the list: legacy mode (any leaf accepted, unencrypted). With at least one, only matching leaves are accepted and the connection is encrypted.",
    th_guid:"GUID", link_leaf_name_ph:"room name", link_leaf_guid_ph:"guid",

    sec_h:"Security", sec_sub:"Anti-flood, anti-bot and captcha protections.",
    sec_warn:"⚠️ Requires <b>restarting the server</b>. If you don't know what a value does, it's best to leave it as is.",
    sec_conn_h:"🚪 Connections",
    sec_maxnew:"Max new connections per IP", sec_window:"Counting window (sec)",
    sec_floodthr:"Flood ban threshold", sec_floodban:"Flood ban duration (sec)",
    sec_maxconc:"Max simultaneous connections per IP", sec_maxraw:"Max raw connections per IP (anti-Slowloris, 0=unlimited)", sec_handshake:"Max login time (sec)", sec_idle:"Max idle time (sec)",
    sec_names_h:"🏷️ Names & logins",
    sec_minname:"Min name length", sec_maxname:"Max name length",
    sec_maxfail:"Max failed logins", sec_failwin:"Failed login window (sec)", sec_failban:"Failed login ban (sec)",
    sec_rejectspam:"Reject spam bots automatically",
    sec_captcha_h:"🤖 Captcha", sec_captcha_on:"Ask new IPs for a captcha",
    sec_captcha_exp:"Captcha expiration (sec)", sec_captcha_att:"Allowed attempts",

    proxy_h:"Trusted proxies", proxy_sub:"For when your server sits behind a proxy (Cloudflare, nginx, etc.).",
    proxy_note:"Only IPs on this list may report the visitor's real IP (via <code>X-Forwarded-For</code>/<code>X-Real-IP</code> headers). Applies to web clients only. Localhost (127.0.0.1) is always trusted. Changes apply instantly.",
    toast_proxy_add:"Proxy added", toast_proxy_rem:"Proxy removed",

    perm_h:"Command permissions", perm_sub:"Minimum rank required to run each command. Applies instantly.",
    perm_search:"🔎 Search command…", th_command:"Command", th_minrank:"Minimum rank",
    perm_change:"Change…", perm_custom:"custom", perm_reset:"Reset",
    toast_perm_upd:"Permission updated", toast_perm_reset:"Permission reset",

    cfg_h:"Advanced config", cfg_sub:"Raw editor for the <code>astra.toml</code> file. For advanced users only.",
    cfg_warn:"⚠️ A mistake here can stop the server from starting. For everyday things (room options, greetings, bans) use the other tabs. Requires a <b>restart</b> to apply.",
    cfg_reload:"Reload",

    con_h:"Console", con_sub:"Run any command as Owner.",
    con_note:"Examples: <code>/ban Pedro</code> · <code>/announce hi everyone</code> · <code>/roomflags</code> · <code>/addline 0, text</code>",
    con_ph:"/command args", con_run:"Run",

    motd_h:"Join message (MOTD)", motd_sub:"Shown to each person when they join the room.",
    motd_note:"One line per message. Placeholders: <code>+n</code> = joining user's name · <code>+rn</code> = room name · <code>+uc</code> = connected users · <code>+ip</code> = IP. Leave it empty to show nothing.",
    motd_ph:"Welcome +n to +rn!\nEnjoy your stay :)",
    motd_saved:"MOTD saved.",

    tpl_h:"System texts", tpl_sub:"Customize (or translate) the moderation messages people see.",
    tpl_note:"Edit the text after the <code>=</code> on each line (format <code>key = text</code>). Placeholders: <code>+n</code> = user · <code>+a</code> = admin · <code>+l</code> = level · <code>+i</code> = ident. To restore a text, set it back to the original.",
    tpl_warn:"All the messages the server shows people through commands are loaded here. The ones with placeholders (like <code>+n</code>) insert live values — keep the placeholder if you want that data to appear.",
    tpl_saved:"Texts saved ({0} applied).",
  }
};
function t(k, ...args){
  const tb = I18N[LANG] || I18N.es;
  let s = (k in tb) ? tb[k] : (I18N.es[k] != null ? I18N.es[k] : k);
  args.forEach((v,i)=>{ s = s.split("{"+i+"}").join(v); });
  return s;
}
const LVL={
  es:{anonymous:"Anónimo",regular:"Regular",voice:"Voz",moderator:"Moderador",admin:"Administrador",owner:"Dueño",system:"Sistema"},
  en:{anonymous:"Anonymous",regular:"Regular",voice:"Voice",moderator:"Moderator",admin:"Administrator",owner:"Owner",system:"System"}
};
function lvlName(n){ return (LVL[LANG]||LVL.es)[n] || n; }
const ACT={
  es:{block:"Bloquear",kick:"Expulsar",ban:"Banear",announce:"Anunciar"},
  en:{block:"Block",kick:"Kick",ban:"Ban",announce:"Announce"}
};
function actName(a){ return (ACT[LANG]||ACT.es)[a] || a; }
const FLAG={
  es:{
    caps:["Bloquear mayúsculas","Pasa a minúsculas los mensajes TODO EN MAYÚSCULAS"],
    anon:["Vigilar anónimos","Monitorea usuarios sin archivos compartidos"],
    general:["Chat general","Permite el chat público de la sala"],
    audios:["Mensajes de voz","Permite enviar audios"],
    buzzes:["Zumbidos","Permite mandar nudges / zumbidos"],
    scribbles:["Dibujos","Permite enviar scribbles (dibujos)"],
    colors:["Texto con color","Permite mensajes con colores"],
    sharefiles:["Vigilar archivos","Monitorea la compartición de archivos"],
    roomsearch:["Búsqueda de salas","Anuncia la sala en el buscador (UDP)"],
    avatars:["Avatares","Permite avatares de usuario"],
    stealth:["Modo sigilo","Oculta la identidad del admin en sus acciones"],
    clock:["Reloj","Muestra la hora en la sala"],
    idle:["Inactividad","Marca a los usuarios inactivos"],
  },
  en:{
    caps:["Block caps","Lowercases ALL-CAPS messages"],
    anon:["Watch anonymous","Monitors users with no shared files"],
    general:["General chat","Enables the room's public chat"],
    audios:["Voice messages","Allows sending audio"],
    buzzes:["Buzzes","Allows sending nudges / buzzes"],
    scribbles:["Scribbles","Allows sending scribbles (drawings)"],
    colors:["Colored text","Allows colored messages"],
    sharefiles:["Watch files","Monitors file sharing"],
    roomsearch:["Room search","Announces the room in the search (UDP)"],
    avatars:["Avatars","Allows user avatars"],
    stealth:["Stealth mode","Hides the admin's identity in their actions"],
    clock:["Clock","Shows the time in the room"],
    idle:["Idle","Flags idle users"],
  }
};
function flagInfo(n){ return (FLAG[LANG]||FLAG.es)[n] || [n,""]; }

const TABS = [
  {gk:"g_principal", items:[
    {id:"inicio", icon:"📊", k:"nav_inicio"},
    {id:"usuarios", icon:"👥", k:"nav_usuarios"},
    {id:"cuentas", icon:"🎫", k:"nav_cuentas"},
  ]},
  {gk:"g_moderacion", items:[
    {id:"baneos", icon:"🚫", k:"nav_baneos"},
    {id:"filtros", icon:"🧹", k:"nav_filtros"},
    {id:"bienvenidas", icon:"👋", k:"nav_bienvenidas"},
  ]},
  {gk:"g_sala", items:[
    {id:"sala", icon:"⚙️", k:"nav_sala"},
    {id:"motd", icon:"📢", k:"nav_motd"},
    {id:"avatares", icon:"🖼️", k:"nav_avatares"},
  ]},
  {gk:"g_avanzado", items:[
    {id:"servidor", icon:"🖥️", k:"nav_servidor"},
    {id:"enlace", icon:"🔗", k:"nav_enlace"},
    {id:"seguridad", icon:"🛡️", k:"nav_seguridad"},
    {id:"proxies", icon:"🌐", k:"nav_proxies"},
    {id:"permisos", icon:"🔑", k:"nav_permisos"},
    {id:"plantillas", icon:"💬", k:"nav_plantillas"},
    {id:"config", icon:"📝", k:"nav_config"},
    {id:"consola", icon:"⌨️", k:"nav_consola"},
  ]},
];
// Pestañas que NO se auto-refrescan (tienen formularios que se borrarían al
// re-renderizar mientras el admin escribe).
const STATIC = new Set(["consola","config","servidor","enlace","seguridad","permisos","proxies","avatares","motd","plantillas"]);

/* ============================ helpers ============================ */
async function api(path, opts={}) {
  opts.headers = opts.headers || {};
  if (TOKEN) opts.headers["Authorization"] = "Bearer " + TOKEN;
  return fetch(path, opts);
}
async function cmd(line) {
  const r = await api("/admin/cmd", {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({cmd:line})});
  if (!r.ok) return ["(error)"];
  const j = await r.json();
  return j.output || [];
}
function esc(s){return (s==null?"":""+s).replace(/[&<>"]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));}
function lvlClass(l){return l>=100?"owner":l>=80?"admin":l>=50?"mod":l>=2?"voice":"";}
function fmtUptime(sec){const d=Math.floor(sec/86400),h=Math.floor(sec/3600)%24,m=Math.floor(sec/60)%60;return (d?d+"d ":"")+h+"h "+m+"m";}

// ¿El usuario está escribiendo en algún campo de la vista? El re-render del
// auto-refresh destruye el DOM (borra lo tipeado y saca el foco), así que el
// poll se saltea mientras haya un input/textarea/select enfocado en #view.
function isEditingView(){
  const a=document.activeElement; if(!a) return false;
  if(!/^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) return false;
  const v=document.getElementById("view");
  return !!(v && v.contains(a));
}

function toast(msg, kind){
  if(!msg) return;
  const el=document.createElement("div");
  el.className="toast "+(kind||"");
  el.textContent=msg;
  document.getElementById("toasts").appendChild(el);
  requestAnimationFrame(()=>el.classList.add("show"));
  setTimeout(()=>{ el.classList.remove("show"); setTimeout(()=>el.remove(),300); }, 2800);
}

async function run(line, okMsg){
  const out = await cmd(line);
  if(TAB==="consola") appendConsole("> "+line+"\n"+out.join("\n")+"\n");
  if(okMsg!==false) toast(okMsg || (out && out[0] ? out[0] : t("common_done")), "ok");
  await refresh();
  return out;
}

/* ============================ i18n / idioma ============================ */
function initLang(){
  const saved = sessionStorage.getItem("astra_lang");
  if(saved){ LANG = saved; return; }
  const nav = (navigator.language || "es").toLowerCase();
  LANG = nav.startsWith("en") ? "en" : "es";
}
function setLang(l){
  LANG = l;
  sessionStorage.setItem("astra_lang", l);
  applyChrome();
  if(!document.getElementById("app").classList.contains("hidden")){
    buildNav(); render(); updateHdr();
  }
}
function applyChrome(){
  const g=(id)=>document.getElementById(id);
  document.documentElement.lang = LANG;
  g("langBtn").textContent = LANG.toUpperCase();
  g("refreshBtn").title = t("chrome_refresh");
  g("refreshBtn").setAttribute("aria-label", t("chrome_refresh"));
  g("logoutBtn").textContent = t("chrome_logout");
  g("menuBtn").setAttribute("aria-label", t("chrome_menu"));
  g("loginTitle").textContent = t("login_title");
  g("loginSub").textContent = t("login_sub");
  g("pw").placeholder = t("login_pw");
  g("loginBtn").textContent = t("login_btn");
  g("langLink").textContent = t("login_switch");
}
function updateHdr(){
  const s = STATE.server || {};
  document.getElementById("hdrStat").textContent =
    `${s.room||""} · ${s.users||0} ${t("hdr_online")} · ${t("hdr_peak")} ${s.peak||0} · ${s.bans||0} ${t("hdr_bans")} · ${fmtUptime(s.uptime||0)}`;
}

async function refresh() {
  const r = await api("/admin/state");
  if (r.status === 401) { logout(); return; }
  STATE = await r.json();
  updateHdr();
  render();
}

function buildNav(){
  const nav=document.getElementById("nav");
  nav.innerHTML = TABS.map(sec=>
    `<div class="navgroup"><div class="navtitle">${esc(t(sec.gk))}</div>`+
    sec.items.map(it=>`<button class="navitem${it.id===TAB?' active':''}" data-tab="${it.id}"><span class="ni-ic">${it.icon}</span><span>${esc(t(it.k))}</span></button>`).join("")+
    `</div>`).join("");
  nav.querySelectorAll(".navitem").forEach(b=>b.onclick=()=>setTab(b.dataset.tab));
}
function setTab(id){ TAB=id; closeDrawer(); buildNav(); render(); window.scrollTo(0,0); }
function openDrawer(){ document.getElementById("side").classList.add("open"); document.getElementById("backdrop").classList.add("show"); }
function closeDrawer(){ document.getElementById("side").classList.remove("open"); document.getElementById("backdrop").classList.remove("show"); }

function render(){
  const map = {
    inicio:renderInicio, usuarios:renderUsuarios, cuentas:renderCuentas,
    baneos:renderBaneos, filtros:renderFiltros, bienvenidas:renderBienvenidas,
    sala:renderSala, motd:renderMotd, avatares:renderAvatares, servidor:renderServidor,
    enlace:renderEnlace, seguridad:renderSeguridad, proxies:renderProxies,
    permisos:renderPermisos, plantillas:renderPlantillas, config:renderConfig, consola:renderConsola
  };
  document.getElementById("view").innerHTML = (map[TAB] || renderInicio)();
  wire();
}

/* ---------------- Principal ---------------- */
function renderInicio(){
  const s = STATE.server||{};
  const tiles = [
    [t("tile_room"), esc(s.room)], [t("tile_bot"), esc(s.bot)],
    [t("tile_online"), s.users], [t("tile_peak"), s.peak],
    [t("tile_total"), s.total], [t("tile_bans"), s.bans],
    [t("tile_uptime"), fmtUptime(s.uptime||0)],
  ];
  return `<div class="cardhead"><h2>${t("inicio_h")}</h2><p class="sub">${t("inicio_sub")}</p></div>
    <div class="tiles">${tiles.map(x=>`<div class="tile"><span class="tl">${x[0]}</span><span class="tv">${x[1]}</span></div>`).join("")}</div>
    <div class="card"><h3>${t("inicio_topic_h")}</h3>
      <label class="fld"><span>${t("inicio_topic_l")}</span><div class="inline"><input id="topicIn" value="${esc(s.topic)}"><button class="btn primary" id="topicSet">${t("common_save")}</button></div></label>
      <label class="fld" style="margin-bottom:0"><span>${t("inicio_status_l")}</span><div class="inline"><input id="statusIn" value="${esc(s.status)}" placeholder="${t("inicio_status_ph")}"><button class="btn" id="statusSet">${t("common_save")}</button></div></label>
    </div>`;
}

function renderUsuarios(){
  const us = STATE.users||[];
  const cards = us.map(u=>{
    const muzAct = u.muzzled ? "unmuzzle" : "muzzle";
    const muzLbl = u.muzzled ? t("u_unmute") : t("u_mute");
    return `<div class="ucard">
      <div class="uhead"><span class="badge ${lvlClass(u.level)}">${esc(lvlName(u.levelName))}</span>
        <b class="uname">${esc(u.name)}</b>
        ${u.muzzled?`<span class="chip warn">${t("u_muted")}</span>`:''}</div>
      <div class="umeta">${esc(u.ip)} · ${t("u_room")} ${u.vroom} · ${u.files||0} ${t("u_files")}${u.version?` · <span class="mut">${esc(u.version)}</span>`:''}</div>
      <div class="uactions">
        <button class="btn sm" data-act="whois" data-n="${esc(u.name)}">${t("u_info")}</button>
        <button class="btn sm" data-act="kick" data-n="${esc(u.name)}">${t("u_kick")}</button>
        <button class="btn sm danger" data-act="ban" data-n="${esc(u.name)}">${t("u_ban")}</button>
        <button class="btn sm" data-act="${muzAct}" data-n="${esc(u.name)}">${muzLbl}</button>
        <select class="sel sm" data-grant="${esc(u.name)}">
          <option value="">${t("u_changerank")}</option>
          <option value="voice">${t("u_to_voice")}</option>
          <option value="moderator">${t("u_to_mod")}</option>
          <option value="admin">${t("u_to_admin")}</option>
          <option value="revoke">${t("u_remrank")}</option>
        </select>
      </div></div>`;
  }).join("");
  return `<div class="cardhead"><h2>${t("users_h")}</h2><p class="sub">${t("users_sub", us.length)}</p></div>
    <div class="ucards">${cards||`<div class="empty">${t("users_empty")}</div>`}</div>`;
}

function renderCuentas(){
  const a = (STATE.accounts||[]).map(x=>`<tr><td><b class="badge ${lvlClass(x.level)}">${esc(lvlName(x.levelName))}</b></td><td>${esc(x.name)}</td></tr>`).join("");
  return `<div class="cardhead"><h2>${t("accounts_h")}</h2><p class="sub">${t("accounts_sub",(STATE.accounts||[]).length)}</p></div>
    <div class="note">${t("accounts_note")}</div>
    <div class="card"><div class="scroll"><table class="tbl"><thead><tr><th>${t("th_rank")}</th><th>${t("th_name")}</th></tr></thead>
    <tbody>${a||`<tr><td colspan=2 class=mut>${t("accounts_empty")}</td></tr>`}</tbody></table></div></div>`;
}

/* ---------------- Moderación ---------------- */
function renderBaneos(){
  const bans = (STATE.bans||[]).map(b=>`<tr><td>${esc(b.name)||'<span class=mut>—</span>'}</td><td class="mut">${esc(b.ip)}</td>
    <td style="text-align:right"><button class="btn sm" data-act2="unban" data-n="${b.ident}">${t("common_remove")}</button></td></tr>`).join("");
  const rb = (STATE.rangeBans||[]).map(p=>`<span class="pill">${esc(p)} <a href="#" data-runban="${esc(p)}">×</a></span>`).join("");
  const ab = (STATE.asnBans||[]).map(a=>`<span class="pill">${t("asn_pill",a)} <a href="#" data-unasn="${a}">×</a></span>`).join("");
  return `<div class="cardhead"><h2>${t("bans_h")}</h2><p class="sub">${t("bans_sub")}</p></div>
    <div class="card"><h3>${t("bans_users_h")} <span class="chip">${(STATE.bans||[]).length}</span></h3>
      <div class="scroll"><table class="tbl"><thead><tr><th>${t("th_name")}</th><th>IP</th><th></th></tr></thead>
      <tbody>${bans||`<tr><td colspan=3 class=mut>${t("bans_users_empty")}</td></tr>`}</tbody></table></div>
      <div class="rowend"><button class="btn danger" id="clearBans">${t("bans_clear")}</button></div></div>
    <div class="card"><h3>${t("bans_range_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("bans_range_desc")}</p>
      <div>${rb||`<span class=mut>${t("common_none")}</span>`}</div>
      <div class="inline" style="margin-top:10px"><input id="rbIn" placeholder="1.2.3."><button class="btn" id="rbAdd">${t("common_add")}</button></div></div>
    <div class="card"><h3>${t("bans_asn_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("bans_asn_desc")}</p>
      <div>${ab||`<span class=mut>${t("common_none")}</span>`}</div>
      <div class="inline" style="margin-top:10px"><input id="abIn" placeholder="${t("bans_asn_ph")}"><button class="btn" id="abAdd">${t("common_add")}</button></div></div>`;
}

function renderFiltros(){
  const f = (STATE.filters||[]).map((x,i)=>`<tr><td>${i}</td><td>${esc(x.pattern)}</td><td><span class="chip">${esc(actName(x.action))}</span></td>
    <td style="text-align:right"><button class="btn sm danger" data-remfilter="${esc(x.pattern)}">${t("common_remove")}</button></td></tr>`).join("");
  return `<div class="cardhead"><h2>${t("filters_h")}</h2><p class="sub">${t("filters_sub")}</p></div>
    <div class="note">${t("filters_note")}</div>
    <div class="card"><h3>${t("filters_active_h")}</h3>
      <div class="scroll"><table class="tbl"><thead><tr><th>#</th><th>${t("th_word")}</th><th>${t("th_action")}</th><th></th></tr></thead>
      <tbody>${f||`<tr><td colspan=4 class=mut>${t("filters_empty")}</td></tr>`}</tbody></table></div>
      <div class="rowend">
        <input id="fpat" placeholder="${t("filters_ph")}" style="flex:1;min-width:150px">
        <select id="fact" class="sel"><option value="block">${actName("block")}</option><option value="kick">${actName("kick")}</option><option value="ban">${actName("ban")}</option><option value="announce">${actName("announce")}</option></select>
        <button class="btn primary" id="faddBtn">${t("filters_add")}</button></div></div>`;
}

function renderBienvenidas(){
  const on = STATE.greetsEnabled;
  const greets = (STATE.greets||[]).map((g,i)=>`<tr><td>${i}</td><td>${esc(g)}</td>
    <td style="text-align:right"><button class="btn sm danger" data-remgreet="${i}">${t("common_remove")}</button></td></tr>`).join("");
  return `<div class="cardhead"><h2>${t("greets_h")}</h2><p class="sub">${t("greets_sub")}<b style="color:${on?'var(--ok)':'var(--mut)'}">${on?t("greets_on"):t("greets_off")}</b>.</p></div>
    <div class="note">${t("greets_note")}</div>
    <div class="card"><div class="scroll"><table class="tbl"><thead><tr><th>#</th><th>${t("th_message")}</th><th></th></tr></thead>
      <tbody>${greets||`<tr><td colspan=3 class=mut>${t("greets_empty")}</td></tr>`}</tbody></table></div>
      <div class="rowend">
        <input id="greetIn" placeholder="${t("greets_ph")}" style="flex:1;min-width:150px">
        <button class="btn primary" id="greetAdd">${t("common_add")}</button>
        <button class="btn" id="greetToggle">${on?t("greets_disable"):t("greets_enable")}</button></div></div>`;
}

/* ---------------- Sala ---------------- */
function renderSala(){
  const flags = (STATE.flags||[]).map(f=>{
    const [lbl,desc]=flagInfo(f.name);
    return `<div class="flag"><div><div class="fn">${esc(lbl)}</div>${desc?`<div class="fd">${esc(desc)}</div>`:''}</div>
      <label class="switch"><input type="checkbox" data-flagtoggle="${esc(f.name)}" ${f.value?'checked':''}><span class="slider"></span></label></div>`;
  }).join("");
  return `<div class="cardhead"><h2>${t("sala_h")}</h2><p class="sub">${t("sala_sub")}</p></div>
    <div class="flags">${flags||`<div class="empty">${t("sala_empty")}</div>`}</div>`;
}

function renderAvatares(){
  return `<div class="cardhead"><h2>${t("av_h")}</h2><p class="sub">${t("av_sub")}</p></div>
    <div class="card"><h3>${t("av_room_h")}</h3>
      <p class="sub" style="margin-bottom:12px">${t("av_room_desc")}</p>
      <div class="avbox"><img id="avImgServer" class="avimg" alt="">
        <div class="avside"><input type="file" id="avFileServer" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateServer">${t("av_upload")}</button></div></div></div>
    <div class="card"><h3>${t("av_def_h")}</h3>
      <p class="sub" style="margin-bottom:12px">${t("av_def_desc")}</p>
      <div class="avbox"><img id="avImgDefault" class="avimg" alt="">
        <div class="avside"><input type="file" id="avFileDefault" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateDefault">${t("av_upload")}</button></div></div></div>`;
}

/* ---------------- Avanzado ---------------- */
async function loadConfig(force){
  if(CONFIG && !force) return CONFIG;
  const r = await api("/admin/config");
  CONFIG = r.ok ? await r.json() : {};
  return CONFIG;
}
async function postConfig(c){
  const r = await api("/admin/config", {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify(c)});
  if(r.ok){ CONFIG=null; toast(t("saved_restart"),"ok"); }
  else { const j = await r.json().catch(()=>({error:"error"})); toast(t("err_prefix")+(j.error||t("err_save")),"err"); }
}

function renderServidor(){
  return `<div class="cardhead"><h2>${t("srv_h")}</h2><p class="sub">${t("srv_sub")}</p></div>
    <div class="warnbox">${t("restart_note")}</div>
    <div class="card">
      <label class="fld"><span>${t("srv_roomname")}</span><input id="cfgRoomName"></label>
      <label class="fld"><span>${t("srv_topic")}</span><input id="cfgRoomTopic"></label>
      <label class="fld"><span>${t("srv_bot")}</span><input id="cfgBotName"></label>
      <div class="grid2">
        <label class="fld"><span>${t("srv_port")}</span><input id="cfgPort" type="number"></label>
        <label class="fld"><span>${t("srv_webport")}</span><input id="cfgWebPort" type="number"></label>
      </div>
      <label class="fld"><span>${t("srv_ownerpw")}</span><input id="cfgOwnerPw" type="text"></label>
      <div class="grid2">
        <label class="fld"><span>${t("srv_lang")}</span><input id="cfgLanguage" type="number"></label>
        <label class="fld"><span>${t("srv_datadir")}</span><input id="cfgDataDir"></label>
      </div>
      <p class="sub" style="margin:-4px 0 10px">${t("srv_override_hint")}</p>
      <label class="check"><input type="checkbox" id="cfgWebEnabled"> ${t("srv_webon")}</label>
      <label class="check"><input type="checkbox" id="cfgAllowReg"> ${t("srv_allowreg")}</label>
      <label class="check"><input type="checkbox" id="cfgRoomsearch"> ${t("srv_roomsearch")}</label>
      <label class="fld"><span>${t("srv_seedurl")}</span><input id="cfgSeedUrl" placeholder="http://chatrooms.mywire.org/rooms.json"><small class="sub">${t("srv_seedurl_hint")}</small></label>
      <div class="rowend"><button class="btn primary" id="cfgSrvSave">${t("common_save_changes")}</button></div>
    </div>`;
}
async function fillServerCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  g("cfgRoomName").value=c.room_name||""; g("cfgRoomTopic").value=c.room_topic||"";
  g("cfgBotName").value=c.bot_name||""; g("cfgPort").value=c.port||0; g("cfgWebPort").value=c.web_port||0;
  g("cfgOwnerPw").value=c.owner_password||""; g("cfgLanguage").value=c.language||0; g("cfgDataDir").value=c.data_dir||"";
  g("cfgWebEnabled").checked=!!c.web_enabled; g("cfgAllowReg").checked=!!c.allow_registration; g("cfgRoomsearch").checked=!!c.roomsearch;
  g("cfgSeedUrl").value=c.seed_url||"";
}
async function saveServerCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  c.room_name=g("cfgRoomName").value; c.room_topic=g("cfgRoomTopic").value; c.bot_name=g("cfgBotName").value;
  c.port=parseInt(g("cfgPort").value)||0; c.web_port=parseInt(g("cfgWebPort").value)||0;
  c.owner_password=g("cfgOwnerPw").value; c.language=parseInt(g("cfgLanguage").value)||0; c.data_dir=g("cfgDataDir").value;
  c.web_enabled=g("cfgWebEnabled").checked; c.allow_registration=g("cfgAllowReg").checked; c.roomsearch=g("cfgRoomsearch").checked;
  c.seed_url=g("cfgSeedUrl").value.trim();
  await postConfig(c);
}

function renderEnlace(){
  return `<div class="cardhead"><h2>${t("link_h")}</h2><p class="sub">${t("link_sub")}</p></div>
    <div class="warnbox">${t("link_warn")}</div>
    <div class="card">
      <label class="check"><input type="checkbox" id="cfgLinkHub"> ${t("link_enable")}</label>
      <label class="fld"><span>${t("link_guid")}</span><input id="cfgGuid"></label>
      <h3 style="margin-top:6px">${t("link_leaves_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("link_leaves_desc")}</p>
      <div class="scroll"><table class="tbl" id="cfgLeavesTbl"><thead><tr><th>${t("th_name")}</th><th>${t("th_guid")}</th><th></th></tr></thead><tbody></tbody></table></div>
      <div class="rowend"><input id="cfgLeafName" placeholder="${t("link_leaf_name_ph")}"><input id="cfgLeafGuid" placeholder="${t("link_leaf_guid_ph")}" style="flex:1;min-width:140px"><button class="btn" id="cfgLeafAdd">${t("common_add")}</button></div>
      <div class="rowend"><button class="btn primary" id="cfgLinkSave">${t("common_save_changes")}</button></div>
    </div>`;
}
function renderLeavesTable(leaves){
  const tbody=document.querySelector("#cfgLeavesTbl tbody"); if(!tbody) return;
  tbody.innerHTML=(leaves||[]).map((l,i)=>`<tr><td>${esc(l.name)}</td><td class="mut">${esc(l.guid)}</td>
    <td style="text-align:right"><button class="btn sm danger" data-rmleaf="${i}">${t("common_remove")}</button></td></tr>`).join("")||`<tr><td colspan=3 class=mut>${t("common_none_f")}</td></tr>`;
  tbody.querySelectorAll("[data-rmleaf]").forEach(b=>b.onclick=async()=>{
    const c=await loadConfig(); c.link_trusted_leaves=c.link_trusted_leaves||[];
    c.link_trusted_leaves.splice(parseInt(b.dataset.rmleaf),1); renderLeavesTable(c.link_trusted_leaves);
  });
}
async function fillLinking(){
  const c=await loadConfig();
  document.getElementById("cfgLinkHub").checked=!!c.link_hub_enabled;
  document.getElementById("cfgGuid").value=c.guid||"";
  renderLeavesTable(c.link_trusted_leaves||[]);
}
async function saveLinking(){
  const c=await loadConfig();
  c.link_hub_enabled=document.getElementById("cfgLinkHub").checked;
  c.guid=document.getElementById("cfgGuid").value;
  await postConfig(c);
}

function renderSeguridad(){
  const fld=(id,lbl)=>`<label class="fld"><span>${lbl}</span><input id="${id}" type="number"></label>`;
  return `<div class="cardhead"><h2>${t("sec_h")}</h2><p class="sub">${t("sec_sub")}</p></div>
    <div class="warnbox">${t("sec_warn")}</div>
    <div class="card"><h3>${t("sec_conn_h")}</h3><div class="grid2">
      ${fld("secMaxNew",t("sec_maxnew"))}
      ${fld("secConnWindow",t("sec_window"))}
      ${fld("secFloodThresh",t("sec_floodthr"))}
      ${fld("secFloodBan",t("sec_floodban"))}
      ${fld("secMaxConc",t("sec_maxconc"))}
      ${fld("secMaxRaw",t("sec_maxraw"))}
      ${fld("secHandshake",t("sec_handshake"))}
      ${fld("secIdle",t("sec_idle"))}
    </div></div>
    <div class="card"><h3>${t("sec_names_h")}</h3><div class="grid2">
      ${fld("secMinName",t("sec_minname"))}
      ${fld("secMaxName",t("sec_maxname"))}
      ${fld("secMaxFailed",t("sec_maxfail"))}
      ${fld("secFailedWindow",t("sec_failwin"))}
      ${fld("secFailedBan",t("sec_failban"))}
    </div>
    <label class="check"><input type="checkbox" id="secRejectSpam"> ${t("sec_rejectspam")}</label></div>
    <div class="card"><h3>${t("sec_captcha_h")}</h3>
      <label class="check"><input type="checkbox" id="secCaptchaEnabled"> ${t("sec_captcha_on")}</label>
      <div class="grid2">${fld("secCaptchaExp",t("sec_captcha_exp"))}${fld("secCaptchaAttempts",t("sec_captcha_att"))}</div>
      <div class="rowend"><button class="btn primary" id="cfgAdvSave">${t("common_save_changes")}</button></div></div>`;
}
async function fillAdvanced(){
  const c=await loadConfig(); const s=c.security||{}; const g=(id)=>document.getElementById(id);
  g("secMaxNew").value=s.max_new_connections_per_ip??10; g("secConnWindow").value=s.connection_window_secs??60;
  g("secFloodThresh").value=s.connection_flood_ban_threshold??3; g("secFloodBan").value=s.connection_flood_ban_secs??300;
  g("secMaxConc").value=s.max_concurrent_per_ip??5; g("secMaxRaw").value=s.max_raw_connections_per_ip??30; g("secHandshake").value=s.handshake_timeout_secs??15;
  g("secIdle").value=s.idle_timeout_secs??1800; g("secMinName").value=s.min_name_length??1; g("secMaxName").value=s.max_name_length??30;
  g("secMaxFailed").value=s.max_failed_logins??5; g("secFailedWindow").value=s.failed_login_window_secs??3600;
  g("secFailedBan").value=s.failed_login_ban_secs??3600; g("secRejectSpam").checked=!!s.reject_spam_bots;
  g("secCaptchaEnabled").checked=!!s.captcha_enabled; g("secCaptchaExp").value=s.captcha_expiration_secs??300; g("secCaptchaAttempts").value=s.captcha_max_attempts??3;
}
async function saveAdvanced(){
  const c=await loadConfig(); c.security=c.security||{}; const s=c.security; const g=(id)=>parseInt(document.getElementById(id).value)||0;
  s.max_new_connections_per_ip=g("secMaxNew"); s.connection_window_secs=g("secConnWindow");
  s.connection_flood_ban_threshold=g("secFloodThresh"); s.connection_flood_ban_secs=g("secFloodBan");
  s.max_concurrent_per_ip=g("secMaxConc"); s.max_raw_connections_per_ip=g("secMaxRaw"); s.handshake_timeout_secs=g("secHandshake"); s.idle_timeout_secs=g("secIdle");
  s.min_name_length=g("secMinName"); s.max_name_length=g("secMaxName"); s.max_failed_logins=g("secMaxFailed");
  s.failed_login_window_secs=g("secFailedWindow"); s.failed_login_ban_secs=g("secFailedBan");
  s.reject_spam_bots=document.getElementById("secRejectSpam").checked;
  s.captcha_enabled=document.getElementById("secCaptchaEnabled").checked;
  s.captcha_expiration_secs=g("secCaptchaExp"); s.captcha_max_attempts=g("secCaptchaAttempts");
  await postConfig(c);
}

function renderProxies(){
  const rows=(STATE.trustedProxies||[]).map(ip=>`<span class="pill">${esc(ip)} <a href="#" data-rmproxy="${esc(ip)}">×</a></span>`).join("");
  return `<div class="cardhead"><h2>${t("proxy_h")}</h2><p class="sub">${t("proxy_sub")}</p></div>
    <div class="note">${t("proxy_note")}</div>
    <div class="card"><div>${rows||`<span class=mut>${t("common_none_f")}</span>`}</div>
    <div class="inline" style="margin-top:12px"><input id="proxyIn" placeholder="1.2.3.4"><button class="btn primary" id="proxyAdd">${t("common_add")}</button></div></div>`;
}

function renderPermisos(){
  const rows=(STATE.commandLevels||[]).map(c=>`<tr data-cmdrow="${esc(c.name)}"><td>/${esc(c.name)}</td>
    <td><span class="badge ${lvlClass(c.level)}">${esc(lvlName(c.levelName))}</span> ${c.isOverride?`<span class="chip">${t("perm_custom")}</span>`:''}</td>
    <td style="text-align:right"><select class="sel sm" data-cmdlvl="${esc(c.name)}">
      <option value="">${t("perm_change")}</option><option value="regular">${lvlName("regular")}</option><option value="voice">${lvlName("voice")}</option>
      <option value="moderator">${lvlName("moderator")}</option><option value="admin">${lvlName("admin")}</option><option value="owner">${lvlName("owner")}</option>
      </select>${c.isOverride?` <button class="btn sm" data-cmdreset="${esc(c.name)}">${t("perm_reset")}</button>`:''}</td></tr>`).join("");
  return `<div class="cardhead"><h2>${t("perm_h")}</h2><p class="sub">${t("perm_sub")}</p></div>
    <div class="card"><div class="inline" style="margin-bottom:12px"><input id="permFilter" placeholder="${t("perm_search")}"></div>
    <div class="scroll"><table class="tbl"><thead><tr><th>${t("th_command")}</th><th>${t("th_minrank")}</th><th></th></tr></thead>
    <tbody>${rows||'<tr><td colspan=3 class=mut>—</td></tr>'}</tbody></table></div></div>`;
}

function renderMotd(){
  return `<div class="cardhead"><h2>${t("motd_h")}</h2><p class="sub">${t("motd_sub")}</p></div>
    <div class="note">${t("motd_note")}</div>
    <div class="card"><textarea id="motdEd" spellcheck="false" style="width:100%;height:34vh;font-family:inherit;font-size:14px" placeholder="${esc(t("motd_ph"))}"></textarea>
    <div class="rowend"><button class="btn primary" id="motdSave">${t("common_save")}</button></div></div>`;
}
async function loadMotd(){
  const r=await api("/admin/motd"); const el=document.getElementById("motdEd"); if(!el) return;
  if(r.ok){ const j=await r.json(); el.value=j.text||""; }
}
async function saveMotd(){
  const el=document.getElementById("motdEd");
  const r=await api("/admin/motd",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({text:el.value})});
  if(r.ok) toast(t("motd_saved"),"ok");
  else toast(t("err_prefix")+t("err_save"),"err");
}

function renderPlantillas(){
  return `<div class="cardhead"><h2>${t("tpl_h")}</h2><p class="sub">${t("tpl_sub")}</p></div>
    <div class="note">${t("tpl_note")}</div>
    <div class="warnbox">${t("tpl_warn")}</div>
    <div class="card"><textarea id="tplEd" spellcheck="false" style="width:100%;height:46vh;font-family:ui-monospace,monospace;font-size:12.5px" placeholder="…"></textarea>
    <div class="rowend"><button class="btn primary" id="tplSave">${t("common_save")}</button></div></div>`;
}
async function loadPlantillas(){
  const r=await api("/admin/template"); const el=document.getElementById("tplEd"); if(!el) return;
  if(r.ok){ const j=await r.json(); el.value=j.text||""; }
}
async function savePlantillas(){
  const el=document.getElementById("tplEd");
  const r=await api("/admin/template",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({text:el.value})});
  if(r.ok){ const j=await r.json().catch(()=>({applied:0})); toast(t("tpl_saved", j.applied!=null?j.applied:0),"ok"); }
  else toast(t("err_prefix")+t("err_save"),"err");
}

function renderConfig(){
  return `<div class="cardhead"><h2>${t("cfg_h")}</h2><p class="sub">${t("cfg_sub")}</p></div>
    <div class="warnbox">${t("cfg_warn")}</div>
    <div class="card"><textarea id="tomlEd" spellcheck="false" style="width:100%;height:50vh;font-family:ui-monospace,monospace;font-size:12.5px" placeholder="…"></textarea>
    <div class="rowend"><button class="btn primary" id="tomlSave">${t("common_save")}</button><button class="btn" id="tomlReload">${t("cfg_reload")}</button></div></div>`;
}
async function loadSettings(){
  const r=await api("/admin/settings"); const el=document.getElementById("tomlEd"); if(!el) return;
  if(r.ok){ const j=await r.json(); el.value=j.toml||""; } else { el.value="# error"; }
}
async function saveSettings(){
  const el=document.getElementById("tomlEd");
  const r=await api("/admin/settings",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({toml:el.value})});
  if(r.ok) toast(t("saved_restart"),"ok");
  else { const j=await r.json().catch(()=>({error:"error"})); toast(t("err_prefix")+(j.error||t("err_save")),"err"); }
}

let CONSOLE_LOG="";
function renderConsola(){
  return `<div class="cardhead"><h2>${t("con_h")}</h2><p class="sub">${t("con_sub")}</p></div>
    <div class="note">${t("con_note")}</div>
    <div class="card"><div id="console-out">${esc(CONSOLE_LOG)}</div>
    <div class="inline" style="margin-top:10px"><input id="cmdIn" placeholder="${t("con_ph")}" autofocus><button class="btn primary" id="cmdRun">${t("con_run")}</button></div></div>`;
}
function appendConsole(x){ CONSOLE_LOG+=x+"\n"; const el=document.getElementById("console-out"); if(el){el.textContent=CONSOLE_LOG; el.scrollTop=el.scrollHeight;} }

async function loadAvatarPreview(kind){
  const img=document.getElementById(kind==="server"?"avImgServer":"avImgDefault"); if(!img) return;
  const r=await api("/admin/avatar/"+kind);
  if(r.ok){ const blob=await r.blob(); img.src=URL.createObjectURL(blob); }
}
function fileToB64(file){
  return new Promise((res,rej)=>{ const rd=new FileReader(); rd.onload=()=>res((rd.result||"").split(",")[1]||""); rd.onerror=rej; rd.readAsDataURL(file); });
}
async function uploadAvatar(kind, fileInputId){
  const input=document.getElementById(fileInputId);
  if(!input.files[0]){ toast(t("av_pick"),"err"); return; }
  const b64=await fileToB64(input.files[0]);
  const r=await api("/admin/avatar",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({kind,data_b64:b64})});
  if(r.ok){ toast(t("av_updated"),"ok"); await loadAvatarPreview(kind); }
  else { const j=await r.json().catch(()=>({error:"error"})); toast(t("err_prefix")+(j.error||t("av_err")),"err"); }
}

function wire(){
  const g=(id)=>document.getElementById(id);
  document.querySelectorAll("[data-act]").forEach(b=>b.onclick=()=>{
    const n=b.dataset.n, a=b.dataset.act;
    if(a==="ban"&&!confirm(t("cf_ban",n)))return;
    const msg={whois:false,kick:t("toast_kicked",n),ban:t("toast_banned",n),muzzle:t("toast_muted",n),unmuzzle:t("toast_unmuted",n)}[a];
    run(`/${a} ${n}`, msg);
  });
  document.querySelectorAll("[data-act2]").forEach(b=>b.onclick=()=>run(`/${b.dataset.act2} ${b.dataset.n}`,t("toast_ban_rem")));
  document.querySelectorAll("[data-grant]").forEach(s=>s.onchange=()=>{
    const n=s.dataset.grant, v=s.value; if(!v) return;
    if(v==="revoke") run(`/revoke ${n}`,t("toast_rank_rem",n)); else run(`/grant ${n} ${v}`,t("toast_rank_upd",n));
  });
  document.querySelectorAll("[data-flagtoggle]").forEach(inp=>inp.onchange=()=>run(`/${inp.dataset.flagtoggle} ${inp.checked?"on":"off"}`, false));
  document.querySelectorAll("[data-remgreet]").forEach(b=>b.onclick=()=>run(`/remgreet ${b.dataset.remgreet}`,t("toast_greet_rem")));
  document.querySelectorAll("[data-remfilter]").forEach(b=>b.onclick=()=>run(`/remfilter ${b.dataset.remfilter}`,t("toast_filter_rem")));
  document.querySelectorAll("[data-runban]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/rangeunban ${a.dataset.runban}`,t("toast_range_unban"));});
  document.querySelectorAll("[data-unasn]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/asnunban ${a.dataset.unasn}`,t("toast_asn_unban"));});
  document.querySelectorAll("[data-cmdlvl]").forEach(s=>s.onchange=()=>{ if(s.value) run(`/cmdlevel ${s.dataset.cmdlvl} ${s.value}`,t("toast_perm_upd")); });
  document.querySelectorAll("[data-cmdreset]").forEach(b=>b.onclick=()=>run(`/cmdlevel ${b.dataset.cmdreset} reset`,t("toast_perm_reset")));
  if(g("topicSet"))g("topicSet").onclick=()=>run(`/topic ${g("topicIn").value}`,t("toast_topic"));
  if(g("statusSet"))g("statusSet").onclick=()=>run(`/status ${g("statusIn").value}`,t("toast_status"));
  if(g("clearBans"))g("clearBans").onclick=()=>{ if(confirm(t("cf_clear"))) run("/clearbans",t("toast_cleared")); };
  if(g("rbAdd"))g("rbAdd").onclick=()=>{ if(g("rbIn").value.trim()) run(`/rangeban ${g("rbIn").value.trim()}`,t("toast_range_ban")); };
  if(g("abAdd"))g("abAdd").onclick=()=>{ if(g("abIn").value.trim()) run(`/asnban ${g("abIn").value.trim()}`,t("toast_asn_ban")); };
  if(g("greetAdd"))g("greetAdd").onclick=()=>{ if(g("greetIn").value.trim()) run(`/addgreet ${g("greetIn").value.trim()}`,t("toast_greet_add")); };
  if(g("greetToggle"))g("greetToggle").onclick=()=>run(`/greets ${STATE.greetsEnabled?"off":"on"}`,t("toast_toggled"));
  if(g("faddBtn"))g("faddBtn").onclick=()=>{ const p=g("fpat").value.trim(); if(p) run(`/addfilter ${p} ${g("fact").value}`,t("toast_filter_add")); };
  if(g("cmdRun")){const rc=()=>{const l=g("cmdIn").value.trim(); if(l){run(l); g("cmdIn").value="";}}; g("cmdRun").onclick=rc; g("cmdIn").onkeydown=e=>{if(e.key==="Enter")rc();};}
  if(g("tomlEd")){ loadSettings(); g("tomlSave").onclick=saveSettings; g("tomlReload").onclick=loadSettings; }
  if(g("motdEd")){ loadMotd(); g("motdSave").onclick=saveMotd; }
  if(g("tplEd")){ loadPlantillas(); g("tplSave").onclick=savePlantillas; }
  if(g("cfgSrvSave")){ fillServerCfg(); g("cfgSrvSave").onclick=saveServerCfg; }
  if(g("cfgLinkSave")){
    fillLinking(); g("cfgLinkSave").onclick=saveLinking;
    g("cfgLeafAdd").onclick=async()=>{
      const c=await loadConfig(); const name=g("cfgLeafName").value.trim(), guid=g("cfgLeafGuid").value.trim();
      if(!name||!guid) return; c.link_trusted_leaves=c.link_trusted_leaves||[]; c.link_trusted_leaves.push({name,guid});
      renderLeavesTable(c.link_trusted_leaves); g("cfgLeafName").value=""; g("cfgLeafGuid").value="";
    };
  }
  if(g("cfgAdvSave")){ fillAdvanced(); g("cfgAdvSave").onclick=saveAdvanced; }
  if(g("proxyAdd")) g("proxyAdd").onclick=async()=>{
    const ip=g("proxyIn").value.trim(); if(!ip) return;
    await api("/admin/proxy/add",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({ip})});
    g("proxyIn").value=""; toast(t("toast_proxy_add"),"ok"); await refresh();
  };
  document.querySelectorAll("[data-rmproxy]").forEach(a=>a.onclick=async e=>{
    e.preventDefault();
    await api("/admin/proxy/remove",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({ip:a.dataset.rmproxy})});
    toast(t("toast_proxy_rem"),"ok"); await refresh();
  });
  if(g("permFilter")) g("permFilter").oninput=()=>{
    const q=g("permFilter").value.toLowerCase();
    document.querySelectorAll("[data-cmdrow]").forEach(tr=>{ tr.style.display=tr.dataset.cmdrow.toLowerCase().includes(q)?"":"none"; });
  };
  if(g("avUpdateServer")){
    loadAvatarPreview("server"); loadAvatarPreview("default");
    g("avUpdateServer").onclick=()=>uploadAvatar("server","avFileServer");
    g("avUpdateDefault").onclick=()=>uploadAvatar("default","avFileDefault");
  }
}

document.getElementById("menuBtn").onclick=openDrawer;
document.getElementById("backdrop").onclick=closeDrawer;
document.getElementById("refreshBtn").onclick=()=>{ CONFIG=null; refresh(); };
document.getElementById("logoutBtn").onclick=logout;
document.getElementById("langBtn").onclick=()=>setLang(LANG==="es"?"en":"es");
document.getElementById("langLink").onclick=e=>{ e.preventDefault(); setLang(LANG==="es"?"en":"es"); };

async function enterApp(){
  document.getElementById("login").classList.add("hidden");
  document.getElementById("app").classList.remove("hidden");
  buildNav();
  await refresh();
  if(!window._poll) window._poll=setInterval(()=>{ if(!STATIC.has(TAB) && !isEditingView()) refresh(); }, 5000);
}
async function login(){
  const pw=document.getElementById("pw").value;
  const r=await fetch("/admin/login",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({password:pw})});
  if(!r.ok){ document.getElementById("loginErr").textContent=t("login_err"); return; }
  const j=await r.json(); TOKEN=j.token; sessionStorage.setItem("astra_token",TOKEN);
  await enterApp();
}
function logout(){ TOKEN=null; sessionStorage.removeItem("astra_token"); location.reload(); }
document.getElementById("loginBtn").onclick=login;
document.getElementById("pw").onkeydown=e=>{ if(e.key==="Enter")login(); };

// Idioma inicial + textos estáticos.
initLang();
applyChrome();

// Auto-login si hay token guardado.
(async()=>{ const tok=sessionStorage.getItem("astra_token"); if(tok){ TOKEN=tok; const r=await api("/admin/state");
  if(r.ok){ await enterApp(); } else { TOKEN=null; sessionStorage.removeItem("astra_token"); } } })();
</script>
</body>
</html>
"####;
