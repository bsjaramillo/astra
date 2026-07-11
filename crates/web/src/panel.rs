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
/// Rediseño 2026-07: en español, mobile-first (la mayoría de los admins lo
/// usan desde el teléfono), con navegación agrupada en un cajón lateral,
/// lenguaje pensado para usuarios no técnicos, toggles/tarjetas en vez de
/// tablas apretadas y notificaciones tipo toast. El contrato con el backend
/// (endpoints `/admin/*`, campos del STATE, comandos slash) es idéntico al
/// del panel anterior — solo cambió la capa de presentación.
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
  .logo{width:28px;height:28px;border-radius:8px;background:linear-gradient(135deg,var(--acc),#ff9d4d);color:#241100;display:flex;align-items:center;justify-content:center;font-weight:800;flex:none}
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

<div id="login">
  <div class="logincard">
    <div class="logo">A</div>
    <h2>Panel de Astra</h2>
    <p class="sub">Ingresá la contraseña de dueño para administrar tu sala.</p>
    <input id="pw" type="password" placeholder="Contraseña de dueño" autofocus>
    <div class="rowend" style="justify-content:center">
      <button class="btn primary" id="loginBtn" style="width:100%">Entrar</button>
    </div>
    <div id="loginErr" class="sub" style="color:var(--danger)"></div>
  </div>
</div>

<div id="app" class="hidden">
<header>
  <button id="menuBtn" class="iconbtn only-mobile" aria-label="Menú">☰</button>
  <div class="brand"><span class="logo">A</span><span class="only-desktop">Astra</span></div>
  <div class="hstats" id="hdrStat">…</div>
  <div class="spacer"></div>
  <button id="refreshBtn" class="iconbtn" title="Actualizar" aria-label="Actualizar">↻</button>
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

const TABS = [
  {g:"Principal", items:[
    {id:"inicio", icon:"📊", label:"Inicio"},
    {id:"usuarios", icon:"👥", label:"Usuarios"},
    {id:"cuentas", icon:"🎫", label:"Cuentas"},
  ]},
  {g:"Moderación", items:[
    {id:"baneos", icon:"🚫", label:"Baneos"},
    {id:"filtros", icon:"🧹", label:"Filtros de palabras"},
    {id:"bienvenidas", icon:"👋", label:"Bienvenidas"},
  ]},
  {g:"Sala", items:[
    {id:"sala", icon:"⚙️", label:"Opciones de sala"},
    {id:"avatares", icon:"🖼️", label:"Avatares"},
  ]},
  {g:"Avanzado", items:[
    {id:"servidor", icon:"🖥️", label:"Servidor"},
    {id:"enlace", icon:"🔗", label:"Enlace de salas"},
    {id:"seguridad", icon:"🛡️", label:"Seguridad"},
    {id:"proxies", icon:"🌐", label:"Proxies"},
    {id:"permisos", icon:"🔑", label:"Permisos de comandos"},
    {id:"config", icon:"📝", label:"Config avanzada"},
    {id:"consola", icon:"⌨️", label:"Consola"},
  ]},
];
// Pestañas que NO se auto-refrescan (tienen formularios que se borrarían al
// re-renderizar mientras el admin escribe).
const STATIC = new Set(["consola","config","servidor","enlace","seguridad","permisos","proxies","avatares"]);

const LVL_ES={anonymous:"Anónimo",regular:"Regular",voice:"Voz",moderator:"Moderador",admin:"Administrador",owner:"Dueño",system:"Sistema"};
function lvlEs(n){return LVL_ES[n]||n;}
const ACT_ES={block:"Bloquear",kick:"Expulsar",ban:"Banear",announce:"Anunciar"};
function actEs(a){return ACT_ES[a]||a;}
const FLAG_ES={
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
};
function flagEs(n){return FLAG_ES[n]||[n,""];}

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

function toast(msg, kind){
  if(!msg) return;
  const t=document.createElement("div");
  t.className="toast "+(kind||"");
  t.textContent=msg;
  document.getElementById("toasts").appendChild(t);
  requestAnimationFrame(()=>t.classList.add("show"));
  setTimeout(()=>{ t.classList.remove("show"); setTimeout(()=>t.remove(),300); }, 2800);
}

async function run(line, okMsg){
  const out = await cmd(line);
  if(TAB==="consola") appendConsole("> "+line+"\n"+out.join("\n")+"\n");
  if(okMsg!==false) toast(okMsg || (out && out[0] ? out[0] : "Listo"), "ok");
  await refresh();
  return out;
}

async function refresh() {
  const r = await api("/admin/state");
  if (r.status === 401) { logout(); return; }
  STATE = await r.json();
  const s = STATE.server || {};
  document.getElementById("hdrStat").textContent =
    `${s.room} · ${s.users} en línea · pico ${s.peak} · ${s.bans} baneos · ${fmtUptime(s.uptime||0)}`;
  render();
}

function buildNav(){
  const nav=document.getElementById("nav");
  nav.innerHTML = TABS.map(sec=>
    `<div class="navgroup"><div class="navtitle">${sec.g}</div>`+
    sec.items.map(it=>`<button class="navitem${it.id===TAB?' active':''}" data-tab="${it.id}"><span class="ni-ic">${it.icon}</span><span>${it.label}</span></button>`).join("")+
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
    sala:renderSala, avatares:renderAvatares, servidor:renderServidor,
    enlace:renderEnlace, seguridad:renderSeguridad, proxies:renderProxies,
    permisos:renderPermisos, config:renderConfig, consola:renderConsola
  };
  document.getElementById("view").innerHTML = (map[TAB] || renderInicio)();
  wire();
}

/* ---------------- Principal ---------------- */
function renderInicio(){
  const s = STATE.server||{};
  const tiles = [
    ["Sala", esc(s.room)], ["Bot", esc(s.bot)],
    ["En línea", s.users], ["Pico", s.peak],
    ["Ingresos totales", s.total], ["Baneos activos", s.bans],
    ["Tiempo activo", fmtUptime(s.uptime||0)],
  ];
  return `<div class="cardhead"><h2>Inicio</h2><p class="sub">Estado general de tu sala, en tiempo real.</p></div>
    <div class="tiles">${tiles.map(t=>`<div class="tile"><span class="tl">${t[0]}</span><span class="tv">${t[1]}</span></div>`).join("")}</div>
    <div class="card"><h3>💬 Tema y estado</h3>
      <label class="fld"><span>Tema de la sala (topic)</span><div class="inline"><input id="topicIn" value="${esc(s.topic)}"><button class="btn primary" id="topicSet">Guardar</button></div></label>
      <label class="fld" style="margin-bottom:0"><span>Estado (mensaje corto)</span><div class="inline"><input id="statusIn" value="${esc(s.status)}" placeholder="ej. sala en mantenimiento"><button class="btn" id="statusSet">Guardar</button></div></label>
    </div>`;
}

function renderUsuarios(){
  const us = STATE.users||[];
  const cards = us.map(u=>{
    const muzAct = u.muzzled ? "unmuzzle" : "muzzle";
    const muzLbl = u.muzzled ? "🔊 Reactivar" : "🔇 Silenciar";
    return `<div class="ucard">
      <div class="uhead"><span class="badge ${lvlClass(u.level)}">${esc(lvlEs(u.levelName))}</span>
        <b class="uname">${esc(u.name)}</b>
        ${u.muzzled?'<span class="chip warn">silenciado</span>':''}</div>
      <div class="umeta">${esc(u.ip)} · sala ${u.vroom} · ${u.files||0} archivos${u.version?` · <span class="mut">${esc(u.version)}</span>`:''}</div>
      <div class="uactions">
        <button class="btn sm" data-act="whois" data-n="${esc(u.name)}">ℹ️ Info</button>
        <button class="btn sm" data-act="kick" data-n="${esc(u.name)}">👢 Expulsar</button>
        <button class="btn sm danger" data-act="ban" data-n="${esc(u.name)}">🚫 Banear</button>
        <button class="btn sm" data-act="${muzAct}" data-n="${esc(u.name)}">${muzLbl}</button>
        <select class="sel sm" data-grant="${esc(u.name)}">
          <option value="">Cambiar rango…</option>
          <option value="voice">→ Voz</option>
          <option value="moderator">→ Moderador</option>
          <option value="admin">→ Administrador</option>
          <option value="revoke">→ Quitar rango</option>
        </select>
      </div></div>`;
  }).join("");
  return `<div class="cardhead"><h2>Usuarios en línea</h2><p class="sub">${us.length} conectado(s). Tocá una acción para moderar.</p></div>
    <div class="ucards">${cards||'<div class="empty">No hay nadie conectado en este momento.</div>'}</div>`;
}

function renderCuentas(){
  const a = (STATE.accounts||[]).map(x=>`<tr><td><b class="badge ${lvlClass(x.level)}">${esc(lvlEs(x.levelName))}</b></td><td>${esc(x.name)}</td></tr>`).join("");
  return `<div class="cardhead"><h2>Cuentas registradas</h2><p class="sub">${(STATE.accounts||[]).length} cuenta(s) guardada(s) con contraseña.</p></div>
    <div class="note">Para dar o quitar rangos usá la pestaña <b>Usuarios</b> (aplica al instante a quien esté conectado). El rango se recuerda cuando la persona vuelve a entrar con su contraseña.</div>
    <div class="card"><div class="scroll"><table class="tbl"><thead><tr><th>Rango</th><th>Nombre</th></tr></thead>
    <tbody>${a||'<tr><td colspan=2 class=mut>No hay cuentas registradas.</td></tr>'}</tbody></table></div></div>`;
}

/* ---------------- Moderación ---------------- */
function renderBaneos(){
  const bans = (STATE.bans||[]).map(b=>`<tr><td>${esc(b.name)||'<span class=mut>—</span>'}</td><td class="mut">${esc(b.ip)}</td>
    <td style="text-align:right"><button class="btn sm" data-act2="unban" data-n="${b.ident}">Quitar</button></td></tr>`).join("");
  const rb = (STATE.rangeBans||[]).map(p=>`<span class="pill">${esc(p)} <a href="#" data-runban="${esc(p)}">×</a></span>`).join("");
  const ab = (STATE.asnBans||[]).map(a=>`<span class="pill">Red AS${a} <a href="#" data-unasn="${a}">×</a></span>`).join("");
  return `<div class="cardhead"><h2>Baneos</h2><p class="sub">Personas y redes bloqueadas de tu sala.</p></div>
    <div class="card"><h3>🚫 Usuarios baneados <span class="chip">${(STATE.bans||[]).length}</span></h3>
      <div class="scroll"><table class="tbl"><thead><tr><th>Nombre</th><th>IP</th><th></th></tr></thead>
      <tbody>${bans||'<tr><td colspan=3 class=mut>No hay usuarios baneados.</td></tr>'}</tbody></table></div>
      <div class="rowend"><button class="btn danger" id="clearBans">Vaciar todos los baneos</button></div></div>
    <div class="card"><h3>📡 Baneos por rango de IP</h3>
      <p class="sub" style="margin-bottom:10px">Bloquea un rango entero de direcciones. Escribí el prefijo, ej. <code>1.2.3.</code></p>
      <div>${rb||'<span class=mut>Ninguno.</span>'}</div>
      <div class="inline" style="margin-top:10px"><input id="rbIn" placeholder="1.2.3."><button class="btn" id="rbAdd">Agregar</button></div></div>
    <div class="card"><h3>🌍 Baneos por red (ASN)</h3>
      <p class="sub" style="margin-bottom:10px">Bloquea una red/proveedor completo por su número ASN.</p>
      <div>${ab||'<span class=mut>Ninguno.</span>'}</div>
      <div class="inline" style="margin-top:10px"><input id="abIn" placeholder="Número de ASN, ej. 12345"><button class="btn" id="abAdd">Agregar</button></div></div>`;
}

function renderFiltros(){
  const f = (STATE.filters||[]).map((x,i)=>`<tr><td>${i}</td><td>${esc(x.pattern)}</td><td><span class="chip">${esc(actEs(x.action))}</span></td>
    <td style="text-align:right"><button class="btn sm danger" data-remfilter="${esc(x.pattern)}">Quitar</button></td></tr>`).join("");
  return `<div class="cardhead"><h2>Filtros de palabras</h2><p class="sub">Reglas que actúan cuando alguien escribe cierta palabra.</p></div>
    <div class="note"><b>¿Qué hace cada acción?</b> · <b>Bloquear</b>: censura el mensaje · <b>Expulsar</b>: echa a quien la use · <b>Banear</b>: la banea · <b>Anunciar</b>: deja pasar el mensaje y manda respuestas automáticas (se editan con la consola: <code>/addline</code>).</div>
    <div class="card"><h3>🧹 Filtros activos</h3>
      <div class="scroll"><table class="tbl"><thead><tr><th>#</th><th>Palabra / patrón</th><th>Acción</th><th></th></tr></thead>
      <tbody>${f||'<tr><td colspan=4 class=mut>No hay filtros.</td></tr>'}</tbody></table></div>
      <div class="rowend">
        <input id="fpat" placeholder="palabra (se admiten * y ?)" style="flex:1;min-width:150px">
        <select id="fact" class="sel"><option value="block">Bloquear</option><option value="kick">Expulsar</option><option value="ban">Banear</option><option value="announce">Anunciar</option></select>
        <button class="btn primary" id="faddBtn">Agregar filtro</button></div></div>`;
}

function renderBienvenidas(){
  const on = STATE.greetsEnabled;
  const greets = (STATE.greets||[]).map((g,i)=>`<tr><td>${i}</td><td>${esc(g)}</td>
    <td style="text-align:right"><button class="btn sm danger" data-remgreet="${i}">Quitar</button></td></tr>`).join("");
  return `<div class="cardhead"><h2>Mensajes de bienvenida</h2><p class="sub">Se muestran a quien entra a la sala. Estado actual: <b style="color:${on?'var(--ok)':'var(--mut)'}">${on?'activados':'desactivados'}</b>.</p></div>
    <div class="note">Podés usar comodines: <code>+n</code> = nombre de quien entra · <code>+rn</code> = nombre de la sala.</div>
    <div class="card"><div class="scroll"><table class="tbl"><thead><tr><th>#</th><th>Mensaje</th><th></th></tr></thead>
      <tbody>${greets||'<tr><td colspan=3 class=mut>No hay mensajes de bienvenida.</td></tr>'}</tbody></table></div>
      <div class="rowend">
        <input id="greetIn" placeholder="¡Bienvenido/a +n a +rn!" style="flex:1;min-width:150px">
        <button class="btn primary" id="greetAdd">Agregar</button>
        <button class="btn" id="greetToggle">${on?'Desactivar todos':'Activar'}</button></div></div>`;
}

/* ---------------- Sala ---------------- */
function renderSala(){
  const flags = (STATE.flags||[]).map(f=>{
    const [lbl,desc]=flagEs(f.name);
    return `<div class="flag"><div><div class="fn">${esc(lbl)}</div>${desc?`<div class="fd">${esc(desc)}</div>`:''}</div>
      <label class="switch"><input type="checkbox" data-flagtoggle="${esc(f.name)}" ${f.value?'checked':''}><span class="slider"></span></label></div>`;
  }).join("");
  return `<div class="cardhead"><h2>Opciones de la sala</h2><p class="sub">Activá o desactivá funciones. Los cambios se aplican al instante.</p></div>
    <div class="flags">${flags||'<div class="empty">Sin opciones.</div>'}</div>`;
}

function renderAvatares(){
  return `<div class="cardhead"><h2>Avatares</h2><p class="sub">Imágenes que usa el servidor.</p></div>
    <div class="card"><h3>🏠 Avatar de la sala</h3>
      <p class="sub" style="margin-bottom:12px">Se envía a cada cliente Ares al entrar y se actualiza en vivo para todos.</p>
      <div class="avbox"><img id="avImgServer" class="avimg" alt="avatar de la sala">
        <div class="avside"><input type="file" id="avFileServer" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateServer">Subir imagen</button></div></div></div>
    <div class="card"><h3>👤 Avatar por defecto</h3>
      <p class="sub" style="margin-bottom:12px">Se asigna a los clientes Ares que no envían su propio avatar dentro de los primeros 10 segundos.</p>
      <div class="avbox"><img id="avImgDefault" class="avimg" alt="avatar por defecto">
        <div class="avside"><input type="file" id="avFileDefault" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateDefault">Subir imagen</button></div></div></div>`;
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
  if(r.ok){ CONFIG=null; toast("Guardado. Reiniciá el servidor para aplicar los cambios.","ok"); }
  else { const j = await r.json().catch(()=>({error:"error"})); toast("Error: "+(j.error||"no se pudo guardar"),"err"); }
}

function renderServidor(){
  return `<div class="cardhead"><h2>Servidor</h2><p class="sub">Datos básicos de tu servidor.</p></div>
    <div class="warnbox">⚠️ Estos cambios se guardan en el archivo de configuración y se aplican al <b>reiniciar el servidor</b>.</div>
    <div class="card">
      <label class="fld"><span>Nombre de la sala</span><input id="cfgRoomName"></label>
      <label class="fld"><span>Tema por defecto</span><input id="cfgRoomTopic"></label>
      <label class="fld"><span>Nombre del bot</span><input id="cfgBotName"></label>
      <div class="grid2">
        <label class="fld"><span>Puerto principal</span><input id="cfgPort" type="number"></label>
        <label class="fld"><span>Puerto web</span><input id="cfgWebPort" type="number"></label>
      </div>
      <label class="fld"><span>Contraseña de dueño</span><input id="cfgOwnerPw" type="text"></label>
      <div class="grid2">
        <label class="fld"><span>Idioma (0 = inglés)</span><input id="cfgLanguage" type="number"></label>
        <label class="fld"><span>Carpeta de datos</span><input id="cfgDataDir"></label>
      </div>
      <label class="check"><input type="checkbox" id="cfgWebEnabled"> Web / clientes ib0t habilitados</label>
      <label class="check"><input type="checkbox" id="cfgAllowReg"> Permitir registro de cuentas</label>
      <label class="check"><input type="checkbox" id="cfgRoomsearch"> Aparecer en la búsqueda de salas (UDP)</label>
      <div class="rowend"><button class="btn primary" id="cfgSrvSave">Guardar cambios</button></div>
    </div>`;
}
async function fillServerCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  g("cfgRoomName").value=c.room_name||""; g("cfgRoomTopic").value=c.room_topic||"";
  g("cfgBotName").value=c.bot_name||""; g("cfgPort").value=c.port||0; g("cfgWebPort").value=c.web_port||0;
  g("cfgOwnerPw").value=c.owner_password||""; g("cfgLanguage").value=c.language||0; g("cfgDataDir").value=c.data_dir||"";
  g("cfgWebEnabled").checked=!!c.web_enabled; g("cfgAllowReg").checked=!!c.allow_registration; g("cfgRoomsearch").checked=!!c.roomsearch;
}
async function saveServerCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  c.room_name=g("cfgRoomName").value; c.room_topic=g("cfgRoomTopic").value; c.bot_name=g("cfgBotName").value;
  c.port=parseInt(g("cfgPort").value)||0; c.web_port=parseInt(g("cfgWebPort").value)||0;
  c.owner_password=g("cfgOwnerPw").value; c.language=parseInt(g("cfgLanguage").value)||0; c.data_dir=g("cfgDataDir").value;
  c.web_enabled=g("cfgWebEnabled").checked; c.allow_registration=g("cfgAllowReg").checked; c.roomsearch=g("cfgRoomsearch").checked;
  await postConfig(c);
}

function renderEnlace(){
  return `<div class="cardhead"><h2>Enlace de salas</h2><p class="sub">Conecta tu sala con otros servidores (Link Hub).</p></div>
    <div class="warnbox">⚠️ Requiere <b>reiniciar el servidor</b>. El Link Hub viaja por el puerto principal (no usa un puerto aparte).</div>
    <div class="card">
      <label class="check"><input type="checkbox" id="cfgLinkHub"> Activar Link Hub</label>
      <label class="fld"><span>GUID del servidor</span><input id="cfgGuid"></label>
      <h3 style="margin-top:6px">🍃 Salas hijas de confianza</h3>
      <p class="sub" style="margin-bottom:10px">Sin ninguna en la lista: modo legado (se acepta cualquier hija, sin cifrar). Con al menos una, solo se aceptan las que coincidan y la conexión se cifra.</p>
      <div class="scroll"><table class="tbl" id="cfgLeavesTbl"><thead><tr><th>Nombre</th><th>GUID</th><th></th></tr></thead><tbody></tbody></table></div>
      <div class="rowend"><input id="cfgLeafName" placeholder="nombre de la sala"><input id="cfgLeafGuid" placeholder="guid" style="flex:1;min-width:140px"><button class="btn" id="cfgLeafAdd">Agregar</button></div>
      <div class="rowend"><button class="btn primary" id="cfgLinkSave">Guardar cambios</button></div>
    </div>`;
}
function renderLeavesTable(leaves){
  const tbody=document.querySelector("#cfgLeavesTbl tbody"); if(!tbody) return;
  tbody.innerHTML=(leaves||[]).map((l,i)=>`<tr><td>${esc(l.name)}</td><td class="mut">${esc(l.guid)}</td>
    <td style="text-align:right"><button class="btn sm danger" data-rmleaf="${i}">Quitar</button></td></tr>`).join("")||'<tr><td colspan=3 class=mut>Ninguna.</td></tr>';
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
  return `<div class="cardhead"><h2>Seguridad</h2><p class="sub">Protecciones anti-flood, anti-bot y captcha.</p></div>
    <div class="warnbox">⚠️ Requiere <b>reiniciar el servidor</b>. Si no sabés qué hace un valor, es mejor dejarlo como está.</div>
    <div class="card"><h3>🚪 Conexiones</h3><div class="grid2">
      ${fld("secMaxNew","Máx. conexiones nuevas por IP")}
      ${fld("secConnWindow","Ventana de conteo (seg)")}
      ${fld("secFloodThresh","Umbral para banear por flood")}
      ${fld("secFloodBan","Duración del ban por flood (seg)")}
      ${fld("secMaxConc","Máx. conexiones simultáneas por IP")}
      ${fld("secHandshake","Tiempo máx. de login (seg)")}
      ${fld("secIdle","Tiempo máx. inactivo (seg)")}
    </div></div>
    <div class="card"><h3>🏷️ Nombres y logins</h3><div class="grid2">
      ${fld("secMinName","Largo mínimo de nombre")}
      ${fld("secMaxName","Largo máximo de nombre")}
      ${fld("secMaxFailed","Máx. logins fallidos")}
      ${fld("secFailedWindow","Ventana de logins fallidos (seg)")}
      ${fld("secFailedBan","Ban por logins fallidos (seg)")}
    </div>
    <label class="check"><input type="checkbox" id="secRejectSpam"> Rechazar bots de spam automáticamente</label></div>
    <div class="card"><h3>🤖 Captcha</h3>
      <label class="check"><input type="checkbox" id="secCaptchaEnabled"> Pedir captcha a las IP nuevas</label>
      <div class="grid2">${fld("secCaptchaExp","Expiración del captcha (seg)")}${fld("secCaptchaAttempts","Intentos permitidos")}</div>
      <div class="rowend"><button class="btn primary" id="cfgAdvSave">Guardar cambios</button></div></div>`;
}
async function fillAdvanced(){
  const c=await loadConfig(); const s=c.security||{}; const g=(id)=>document.getElementById(id);
  g("secMaxNew").value=s.max_new_connections_per_ip??10; g("secConnWindow").value=s.connection_window_secs??60;
  g("secFloodThresh").value=s.connection_flood_ban_threshold??3; g("secFloodBan").value=s.connection_flood_ban_secs??300;
  g("secMaxConc").value=s.max_concurrent_per_ip??5; g("secHandshake").value=s.handshake_timeout_secs??15;
  g("secIdle").value=s.idle_timeout_secs??1800; g("secMinName").value=s.min_name_length??1; g("secMaxName").value=s.max_name_length??30;
  g("secMaxFailed").value=s.max_failed_logins??5; g("secFailedWindow").value=s.failed_login_window_secs??3600;
  g("secFailedBan").value=s.failed_login_ban_secs??3600; g("secRejectSpam").checked=!!s.reject_spam_bots;
  g("secCaptchaEnabled").checked=!!s.captcha_enabled; g("secCaptchaExp").value=s.captcha_expiration_secs??300; g("secCaptchaAttempts").value=s.captcha_max_attempts??3;
}
async function saveAdvanced(){
  const c=await loadConfig(); c.security=c.security||{}; const s=c.security; const g=(id)=>parseInt(document.getElementById(id).value)||0;
  s.max_new_connections_per_ip=g("secMaxNew"); s.connection_window_secs=g("secConnWindow");
  s.connection_flood_ban_threshold=g("secFloodThresh"); s.connection_flood_ban_secs=g("secFloodBan");
  s.max_concurrent_per_ip=g("secMaxConc"); s.handshake_timeout_secs=g("secHandshake"); s.idle_timeout_secs=g("secIdle");
  s.min_name_length=g("secMinName"); s.max_name_length=g("secMaxName"); s.max_failed_logins=g("secMaxFailed");
  s.failed_login_window_secs=g("secFailedWindow"); s.failed_login_ban_secs=g("secFailedBan");
  s.reject_spam_bots=document.getElementById("secRejectSpam").checked;
  s.captcha_enabled=document.getElementById("secCaptchaEnabled").checked;
  s.captcha_expiration_secs=g("secCaptchaExp"); s.captcha_max_attempts=g("secCaptchaAttempts");
  await postConfig(c);
}

function renderProxies(){
  const rows=(STATE.trustedProxies||[]).map(ip=>`<span class="pill">${esc(ip)} <a href="#" data-rmproxy="${esc(ip)}">×</a></span>`).join("");
  return `<div class="cardhead"><h2>Proxies de confianza</h2><p class="sub">Para cuando tu servidor está detrás de un proxy (Cloudflare, nginx, etc.).</p></div>
    <div class="note">Solo las IP de esta lista pueden decir cuál es la IP real del visitante (vía cabeceras <code>X-Forwarded-For</code>/<code>X-Real-IP</code>). Aplica solo a clientes web. La IP local (127.0.0.1) siempre es de confianza. Los cambios se aplican al instante.</div>
    <div class="card"><div>${rows||'<span class=mut>Ninguna.</span>'}</div>
    <div class="inline" style="margin-top:12px"><input id="proxyIn" placeholder="1.2.3.4"><button class="btn primary" id="proxyAdd">Agregar</button></div></div>`;
}

function renderPermisos(){
  const rows=(STATE.commandLevels||[]).map(c=>`<tr data-cmdrow="${esc(c.name)}"><td>/${esc(c.name)}</td>
    <td><span class="badge ${lvlClass(c.level)}">${esc(lvlEs(c.levelName))}</span> ${c.isOverride?'<span class="chip">personalizado</span>':''}</td>
    <td style="text-align:right"><select class="sel sm" data-cmdlvl="${esc(c.name)}">
      <option value="">Cambiar…</option><option value="regular">Regular</option><option value="voice">Voz</option>
      <option value="moderator">Moderador</option><option value="admin">Administrador</option><option value="owner">Dueño</option>
      </select>${c.isOverride?` <button class="btn sm" data-cmdreset="${esc(c.name)}">Restaurar</button>`:''}</td></tr>`).join("");
  return `<div class="cardhead"><h2>Permisos de comandos</h2><p class="sub">Rango mínimo necesario para usar cada comando. Se aplica al instante.</p></div>
    <div class="card"><div class="inline" style="margin-bottom:12px"><input id="permFilter" placeholder="🔎 Buscar comando…"></div>
    <div class="scroll"><table class="tbl"><thead><tr><th>Comando</th><th>Rango mínimo</th><th></th></tr></thead>
    <tbody>${rows||'<tr><td colspan=3 class=mut>—</td></tr>'}</tbody></table></div></div>`;
}

function renderConfig(){
  return `<div class="cardhead"><h2>Config avanzada</h2><p class="sub">Editor del archivo <code>astra.toml</code> en crudo. Solo para usuarios avanzados.</p></div>
    <div class="warnbox">⚠️ Un error acá puede impedir que el servidor arranque. Para lo cotidiano (opciones de sala, bienvenidas, baneos) usá las otras pestañas. Requiere <b>reiniciar</b> para aplicar.</div>
    <div class="card"><textarea id="tomlEd" spellcheck="false" style="width:100%;height:50vh;font-family:ui-monospace,monospace;font-size:12.5px" placeholder="cargando…"></textarea>
    <div class="rowend"><button class="btn primary" id="tomlSave">Guardar</button><button class="btn" id="tomlReload">Recargar</button></div></div>`;
}
async function loadSettings(){
  const r=await api("/admin/settings"); const el=document.getElementById("tomlEd"); if(!el) return;
  if(r.ok){ const j=await r.json(); el.value=j.toml||""; } else { el.value="# no se pudo cargar la configuración"; }
}
async function saveSettings(){
  const el=document.getElementById("tomlEd");
  const r=await api("/admin/settings",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({toml:el.value})});
  if(r.ok) toast("Guardado. Reiniciá el servidor para aplicar.","ok");
  else { const j=await r.json().catch(()=>({error:"error"})); toast("Error: "+(j.error||"no se pudo guardar"),"err"); }
}

let CONSOLE_LOG="";
function renderConsola(){
  return `<div class="cardhead"><h2>Consola</h2><p class="sub">Ejecutá cualquier comando como Dueño.</p></div>
    <div class="note">Ejemplos: <code>/ban Pedro</code> · <code>/announce hola a todos</code> · <code>/roomflags</code> · <code>/addline 0, texto</code></div>
    <div class="card"><div id="console-out">${esc(CONSOLE_LOG)}</div>
    <div class="inline" style="margin-top:10px"><input id="cmdIn" placeholder="/comando argumentos" autofocus><button class="btn primary" id="cmdRun">Ejecutar</button></div></div>`;
}
function appendConsole(t){ CONSOLE_LOG+=t+"\n"; const el=document.getElementById("console-out"); if(el){el.textContent=CONSOLE_LOG; el.scrollTop=el.scrollHeight;} }

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
  if(!input.files[0]){ toast("Elegí una imagen primero.","err"); return; }
  const b64=await fileToB64(input.files[0]);
  const r=await api("/admin/avatar",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({kind,data_b64:b64})});
  if(r.ok){ toast("Imagen actualizada.","ok"); await loadAvatarPreview(kind); }
  else { const j=await r.json().catch(()=>({error:"error"})); toast("Error: "+(j.error||"no se pudo subir"),"err"); }
}

function wire(){
  const g=(id)=>document.getElementById(id);
  // acciones de usuario
  document.querySelectorAll("[data-act]").forEach(b=>b.onclick=()=>{
    const n=b.dataset.n, a=b.dataset.act;
    if(a==="ban"&&!confirm("¿Seguro que querés banear a "+n+"?"))return;
    const msg={whois:false,kick:"Expulsado: "+n,ban:"Baneado: "+n,muzzle:"Silenciado: "+n,unmuzzle:"Reactivado: "+n}[a];
    run(`/${a} ${n}`, msg);
  });
  document.querySelectorAll("[data-act2]").forEach(b=>b.onclick=()=>run(`/${b.dataset.act2} ${b.dataset.n}`,"Baneo quitado"));
  document.querySelectorAll("[data-grant]").forEach(s=>s.onchange=()=>{
    const n=s.dataset.grant, v=s.value; if(!v) return;
    if(v==="revoke") run(`/revoke ${n}`,"Rango quitado a "+n); else run(`/grant ${n} ${v}`,"Rango actualizado: "+n);
  });
  // flags de sala (toggle switch)
  document.querySelectorAll("[data-flagtoggle]").forEach(inp=>inp.onchange=()=>run(`/${inp.dataset.flagtoggle} ${inp.checked?"on":"off"}`, false));
  // moderación / listas
  document.querySelectorAll("[data-remgreet]").forEach(b=>b.onclick=()=>run(`/remgreet ${b.dataset.remgreet}`,"Bienvenida quitada"));
  document.querySelectorAll("[data-remfilter]").forEach(b=>b.onclick=()=>run(`/remfilter ${b.dataset.remfilter}`,"Filtro quitado"));
  document.querySelectorAll("[data-runban]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/rangeunban ${a.dataset.runban}`,"Rango desbloqueado");});
  document.querySelectorAll("[data-unasn]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/asnunban ${a.dataset.unasn}`,"Red desbloqueada");});
  document.querySelectorAll("[data-cmdlvl]").forEach(s=>s.onchange=()=>{ if(s.value) run(`/cmdlevel ${s.dataset.cmdlvl} ${s.value}`,"Permiso actualizado"); });
  document.querySelectorAll("[data-cmdreset]").forEach(b=>b.onclick=()=>run(`/cmdlevel ${b.dataset.cmdreset} reset`,"Permiso restaurado"));
  // inicio
  if(g("topicSet"))g("topicSet").onclick=()=>run(`/topic ${g("topicIn").value}`,"Tema actualizado");
  if(g("statusSet"))g("statusSet").onclick=()=>run(`/status ${g("statusIn").value}`,"Estado actualizado");
  // baneos
  if(g("clearBans"))g("clearBans").onclick=()=>{ if(confirm("¿Vaciar TODOS los baneos? No se puede deshacer.")) run("/clearbans","Baneos vaciados"); };
  if(g("rbAdd"))g("rbAdd").onclick=()=>{ if(g("rbIn").value.trim()) run(`/rangeban ${g("rbIn").value.trim()}`,"Rango bloqueado"); };
  if(g("abAdd"))g("abAdd").onclick=()=>{ if(g("abIn").value.trim()) run(`/asnban ${g("abIn").value.trim()}`,"Red bloqueada"); };
  // bienvenidas
  if(g("greetAdd"))g("greetAdd").onclick=()=>{ if(g("greetIn").value.trim()) run(`/addgreet ${g("greetIn").value.trim()}`,"Bienvenida agregada"); };
  if(g("greetToggle"))g("greetToggle").onclick=()=>run(`/greets ${STATE.greetsEnabled?"off":"on"}`,"Actualizado");
  // filtros
  if(g("faddBtn"))g("faddBtn").onclick=()=>{ const p=g("fpat").value.trim(); if(p) run(`/addfilter ${p} ${g("fact").value}`,"Filtro agregado"); };
  // consola
  if(g("cmdRun")){const rc=()=>{const l=g("cmdIn").value.trim(); if(l){run(l); g("cmdIn").value="";}}; g("cmdRun").onclick=rc; g("cmdIn").onkeydown=e=>{if(e.key==="Enter")rc();};}
  // config avanzada
  if(g("tomlEd")){ loadSettings(); g("tomlSave").onclick=saveSettings; g("tomlReload").onclick=loadSettings; }
  // servidor / enlace / seguridad
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
  // proxies
  if(g("proxyAdd")) g("proxyAdd").onclick=async()=>{
    const ip=g("proxyIn").value.trim(); if(!ip) return;
    await api("/admin/proxy/add",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({ip})});
    g("proxyIn").value=""; toast("Proxy agregado","ok"); await refresh();
  };
  document.querySelectorAll("[data-rmproxy]").forEach(a=>a.onclick=async e=>{
    e.preventDefault();
    await api("/admin/proxy/remove",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({ip:a.dataset.rmproxy})});
    toast("Proxy quitado","ok"); await refresh();
  });
  // permisos: filtro de búsqueda
  if(g("permFilter")) g("permFilter").oninput=()=>{
    const q=g("permFilter").value.toLowerCase();
    document.querySelectorAll("[data-cmdrow]").forEach(tr=>{ tr.style.display=tr.dataset.cmdrow.toLowerCase().includes(q)?"":"none"; });
  };
  // avatares
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

async function enterApp(){
  document.getElementById("login").classList.add("hidden");
  document.getElementById("app").classList.remove("hidden");
  buildNav();
  await refresh();
  if(!window._poll) window._poll=setInterval(()=>{ if(!STATIC.has(TAB)) refresh(); }, 5000);
}
async function login(){
  const pw=document.getElementById("pw").value;
  const r=await fetch("/admin/login",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({password:pw})});
  if(!r.ok){ document.getElementById("loginErr").textContent="Contraseña incorrecta."; return; }
  const j=await r.json(); TOKEN=j.token; sessionStorage.setItem("astra_token",TOKEN);
  await enterApp();
}
function logout(){ TOKEN=null; sessionStorage.removeItem("astra_token"); location.reload(); }
document.getElementById("loginBtn").onclick=login;
document.getElementById("pw").onkeydown=e=>{ if(e.key==="Enter")login(); };

// Auto-login si hay token guardado.
(async()=>{ const t=sessionStorage.getItem("astra_token"); if(t){ TOKEN=t; const r=await api("/admin/state");
  if(r.ok){ await enterApp(); } else { TOKEN=null; sessionStorage.removeItem("astra_token"); } } })();
</script>
</body>
</html>
"####;
