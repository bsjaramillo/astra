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

ws.onmessage = (e) => {
  log("[<] " + e.data);
  const i = e.data.indexOf(":");
  if (i < 0) return;
  const ident = e.data.substring(0, i);
  const args = e.data.substring(i + 1);
  if (ident === "ACK") {
    const [name, room, ver] = args.split(",");
    log("✓ Logueado como " + name + " en " + room, "nick");
  } else if (ident === "TOPIC") {
    log("📌 " + args, "topic");
  } else if (ident === "PUBLIC") {
    const c = args.indexOf(",");
    log("[" + args.substring(0, c) + "] " + args.substring(c + 1));
  } else if (ident === "PM") {
    log("💬 " + args, "pm");
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
pub const ADMIN_HTML: &str = r####"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Astra Admin</title>
<style>
  :root { --bg:#15171c; --panel:#1e2129; --panel2:#262a34; --fg:#dfe3ea; --mut:#8b93a3; --acc:#4f8cff; --danger:#e0555b; --ok:#3fb950; --border:#333a47; }
  * { box-sizing:border-box; }
  body { margin:0; font-family:system-ui,Segoe UI,Roboto,sans-serif; background:var(--bg); color:var(--fg); font-size:14px; }
  header { display:flex; align-items:center; gap:14px; padding:10px 16px; background:var(--panel); border-bottom:1px solid var(--border); }
  header h1 { font-size:16px; margin:0; font-weight:600; }
  header .stat { color:var(--mut); font-size:12px; }
  header .spacer { flex:1; }
  button { background:var(--panel2); color:var(--fg); border:1px solid var(--border); border-radius:6px; padding:6px 10px; cursor:pointer; font-size:13px; }
  button:hover { border-color:var(--acc); }
  button.danger { border-color:#5a2b2e; } button.danger:hover { border-color:var(--danger); color:var(--danger); }
  input, select, textarea { background:var(--bg); color:var(--fg); border:1px solid var(--border); border-radius:6px; padding:6px 8px; font-size:13px; }
  input:focus, textarea:focus { outline:none; border-color:var(--acc); }
  nav { display:flex; gap:4px; padding:8px 16px; background:var(--panel); border-bottom:1px solid var(--border); flex-wrap:wrap; }
  nav button { background:transparent; border:none; padding:8px 12px; border-radius:6px; color:var(--mut); }
  nav button.active { background:var(--panel2); color:var(--fg); }
  main { padding:16px; max-width:1100px; margin:0 auto; }
  .card { background:var(--panel); border:1px solid var(--border); border-radius:10px; padding:14px; margin-bottom:16px; }
  .card h2 { margin:0 0 12px; font-size:14px; font-weight:600; }
  table { width:100%; border-collapse:collapse; }
  th, td { text-align:left; padding:7px 8px; border-bottom:1px solid var(--border); font-size:13px; }
  th { color:var(--mut); font-weight:500; }
  .row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; margin-bottom:8px; }
  .grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:10px; }
  .flag { display:flex; align-items:center; justify-content:space-between; background:var(--panel2); border:1px solid var(--border); border-radius:8px; padding:8px 12px; }
  .pill { font-size:11px; padding:2px 7px; border-radius:10px; background:var(--panel2); color:var(--mut); }
  .lvl-100{color:#ffb454}.lvl-80{color:#e0555b}.lvl-50{color:#4f8cff}.lvl-2{color:#3fb950}
  #login { max-width:340px; margin:12vh auto; }
  #console-out { background:#0d0f13; border:1px solid var(--border); border-radius:8px; padding:10px; height:300px; overflow-y:auto; font-family:ui-monospace,monospace; font-size:12px; white-space:pre-wrap; }
  .muted { color:var(--mut); }
  .hidden { display:none; }
  .tag { font-size:11px; color:var(--mut); margin-left:6px; }
</style>
</head>
<body>

<div id="login" class="card">
  <h2>Astra Admin</h2>
  <p class="muted">Enter the owner password.</p>
  <div class="row">
    <input id="pw" type="password" placeholder="owner password" style="flex:1" autofocus>
    <button id="loginBtn">Log in</button>
  </div>
  <div id="loginErr" class="muted"></div>
</div>

<div id="app" class="hidden">
<header>
  <h1>Astra</h1>
  <span class="stat" id="hdrStat">…</span>
  <div class="spacer"></div>
  <button id="refreshBtn">↻ Refresh</button>
  <button id="logoutBtn">Log out</button>
</header>
<nav id="tabs">
  <button data-tab="dashboard" class="active">Dashboard</button>
  <button data-tab="users">Users</button>
  <button data-tab="bans">Bans</button>
  <button data-tab="room">Room</button>
  <button data-tab="filters">Filters</button>
  <button data-tab="accounts">Accounts</button>
  <button data-tab="settings">Settings</button>
  <button data-tab="console">Console</button>
</nav>
<main id="view"></main>
</div>

<script>
let TOKEN = null;
let STATE = {};
let TAB = "dashboard";

async function api(path, opts={}) {
  opts.headers = opts.headers || {};
  if (TOKEN) opts.headers["Authorization"] = "Bearer " + TOKEN;
  const r = await fetch(path, opts);
  return r;
}
async function cmd(line) {
  const r = await api("/admin/cmd", {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({cmd:line})});
  if (!r.ok) return ["(error)"];
  const j = await r.json();
  return j.output || [];
}
async function refresh() {
  const r = await api("/admin/state");
  if (r.status === 401) { logout(); return; }
  STATE = await r.json();
  const s = STATE.server || {};
  const up = fmtUptime(s.uptime||0);
  document.getElementById("hdrStat").textContent =
    `${s.room} · ${s.users} online · peak ${s.peak} · ${s.bans} bans · up ${up}`;
  render();
}
function fmtUptime(sec){const d=Math.floor(sec/86400),h=Math.floor(sec/3600)%24,m=Math.floor(sec/60)%60;return `${d}d ${h}h ${m}m`;}
function esc(s){return (s==null?"":""+s).replace(/[&<>"]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));}
function lvlClass(l){return l>=100?"lvl-100":l>=80?"lvl-80":l>=50?"lvl-50":l>=2?"lvl-2":"";}

async function run(line, quiet){
  const out = await cmd(line);
  if(!quiet && TAB==="console") appendConsole("> "+line+"\n"+out.join("\n")+"\n");
  await refresh();
  return out;
}

function render(){
  const v = document.getElementById("view");
  v.innerHTML = ({
    dashboard: renderDashboard, users: renderUsers, bans: renderBans,
    room: renderRoom, filters: renderFilters, accounts: renderAccounts, settings: renderSettings, console: renderConsole
  }[TAB] || renderDashboard)();
  wire();
}

function renderDashboard(){
  const s = STATE.server||{};
  return `<div class="card"><h2>Server</h2>
    <div class="grid">
      <div class="flag"><span>Room</span><b>${esc(s.room)}</b></div>
      <div class="flag"><span>Bot</span><b>${esc(s.bot)}</b></div>
      <div class="flag"><span>Users online</span><b>${s.users}</b></div>
      <div class="flag"><span>Peak</span><b>${s.peak}</b></div>
      <div class="flag"><span>Total joins</span><b>${s.total}</b></div>
      <div class="flag"><span>Active bans</span><b>${s.bans}</b></div>
      <div class="flag"><span>Uptime</span><b>${fmtUptime(s.uptime||0)}</b></div>
    </div></div>
    <div class="card"><h2>Topic</h2>
      <div class="row"><input id="topicIn" value="${esc(s.topic)}" style="flex:1"><button id="topicSet">Set</button></div>
      <div class="row"><input id="statusIn" placeholder="room status" value="${esc(s.status)}" style="flex:1"><button id="statusSet">Set status</button></div>
    </div>`;
}

function renderUsers(){
  const rows = (STATE.users||[]).map(u=>`<tr>
    <td>${u.id}</td>
    <td><b class="${lvlClass(u.level)}">${esc(u.name)}</b> ${u.muzzled?'<span class="pill">muzzled</span>':''}</td>
    <td class="muted">${esc(u.levelName)}</td>
    <td class="muted">${esc(u.ip)}</td>
    <td class="muted">${u.vroom}</td>
    <td>
      <button data-act="whois" data-n="${esc(u.name)}">whois</button>
      <button data-act="kick" data-n="${esc(u.name)}">kick</button>
      <button class="danger" data-act="ban" data-n="${esc(u.name)}">ban</button>
      <button data-act="muzzle" data-n="${esc(u.name)}">muzzle</button>
      <select data-grant="${esc(u.name)}">
        <option value="">grant…</option>
        <option value="voice">voice</option><option value="moderator">moderator</option>
        <option value="admin">admin</option><option value="regular">regular</option>
      </select>
    </td></tr>`).join("");
  return `<div class="card"><h2>Users (${(STATE.users||[]).length})</h2>
    <table><thead><tr><th>ID</th><th>Name</th><th>Level</th><th>IP</th><th>Vroom</th><th>Actions</th></tr></thead>
    <tbody>${rows||'<tr><td colspan=6 class=muted>No users online</td></tr>'}</tbody></table></div>`;
}

function renderBans(){
  const bans = (STATE.bans||[]).map(b=>`<tr><td>${b.ident}</td><td>${esc(b.name)}</td><td class="muted">${esc(b.ip)}</td>
    <td><button data-act="unban" data-n="${b.ident}">unban</button></td></tr>`).join("");
  const rb = (STATE.rangeBans||[]).map(p=>`<span class="pill">${esc(p)} <a href="#" data-runban="${esc(p)}">✕</a></span>`).join(" ");
  const ab = (STATE.asnBans||[]).map(a=>`<span class="pill">AS${a} <a href="#" data-unasn="${a}">✕</a></span>`).join(" ");
  return `<div class="card"><h2>Bans (${(STATE.bans||[]).length})</h2>
    <table><thead><tr><th>Ident</th><th>Name</th><th>IP</th><th></th></tr></thead>
    <tbody>${bans||'<tr><td colspan=4 class=muted>No bans</td></tr>'}</tbody></table>
    <div class="row" style="margin-top:10px"><button class="danger" id="clearBans">Clear all bans</button></div></div>
    <div class="card"><h2>Range bans</h2><div class="row">${rb||'<span class=muted>none</span>'}</div>
    <div class="row"><input id="rbIn" placeholder="1.2.3." style="flex:1"><button id="rbAdd">Add range ban</button></div></div>
    <div class="card"><h2>ASN bans</h2><div class="row">${ab||'<span class=muted>none</span>'}</div>
    <div class="row"><input id="abIn" placeholder="ASN number" style="flex:1"><button id="abAdd">Add ASN ban</button></div></div>`;
}

function renderRoom(){
  const flags = (STATE.flags||[]).map(f=>`<div class="flag"><span>${esc(f.name)}</span>
    <button data-flag="${esc(f.name)}" data-on="${f.value}">${f.value?'on':'off'}</button></div>`).join("");
  const greets = (STATE.greets||[]).map((g,i)=>`<tr><td>${i}</td><td>${esc(g)}</td>
    <td><button class="danger" data-remgreet="${i}">remove</button></td></tr>`).join("");
  return `<div class="card"><h2>Room flags</h2><div class="grid">${flags}</div></div>
    <div class="card"><h2>Greets <span class="tag">${STATE.greetsEnabled?'enabled':'disabled'}</span></h2>
    <table><tbody>${greets||'<tr><td class=muted>none</td></tr>'}</tbody></table>
    <div class="row" style="margin-top:8px"><input id="greetIn" placeholder="welcome +n to +rn" style="flex:1"><button id="greetAdd">Add greet</button>
    <button id="greetToggle">${STATE.greetsEnabled?'Disable':'Enable'}</button></div></div>`;
}

function renderFilters(){
  const f = (STATE.filters||[]).map(x=>`<tr><td>${esc(x.pattern)}</td><td class="muted">${esc(x.action)}</td>
    <td><button class="danger" data-remfilter="${esc(x.pattern)}">remove</button></td></tr>`).join("");
  return `<div class="card"><h2>Word filters</h2>
    <table><thead><tr><th>Pattern</th><th>Action</th><th></th></tr></thead>
    <tbody>${f||'<tr><td colspan=3 class=muted>none</td></tr>'}</tbody></table>
    <div class="row" style="margin-top:8px"><input id="fpat" placeholder="pattern (* ? wildcards)">
    <select id="fact"><option value="block">block</option><option value="kick">kick</option><option value="ban">ban</option></select>
    <button id="faddBtn">Add filter</button></div></div>`;
}

function renderAccounts(){
  const a = (STATE.accounts||[]).map(x=>`<tr><td><b class="${lvlClass(x.level)}">${esc(x.name)}</b></td>
    <td class="muted">${esc(x.levelName)}</td></tr>`).join("");
  return `<div class="card"><h2>Registered accounts (${(STATE.accounts||[]).length})</h2>
    <table><thead><tr><th>Name</th><th>Level</th></tr></thead>
    <tbody>${a||'<tr><td colspan=2 class=muted>none</td></tr>'}</tbody></table>
    <p class="muted">Grant/revoke apply to online users (see Users tab).</p></div>`;
}

function renderSettings(){
  return `<div class="card"><h2>Server configuration (astra.toml)</h2>
    <p class="muted">Edit the raw config. Changes are validated and written to the file.
    <b>A server restart is required</b> for startup settings (port, security thresholds, etc.) to take effect.
    Live things (room flags, greets, bans) are better changed from the other tabs.</p>
    <textarea id="tomlEd" spellcheck="false" style="width:100%;height:52vh;font-family:ui-monospace,monospace;font-size:12px" placeholder="loading…"></textarea>
    <div class="row" style="margin-top:8px"><button id="tomlSave">Save to astra.toml</button>
    <button id="tomlReload">Reload</button><span id="tomlMsg" class="muted"></span></div></div>`;
}
async function loadSettings(){
  const r = await api("/admin/settings");
  const el = document.getElementById("tomlEd");
  if(!el) return;
  if(r.ok){ const j = await r.json(); el.value = j.toml || ""; }
  else { el.value = "# failed to load settings"; }
}
async function saveSettings(){
  const el = document.getElementById("tomlEd"); const msg = document.getElementById("tomlMsg");
  const r = await api("/admin/settings", {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({toml:el.value})});
  if(r.ok){ msg.textContent = "✓ Saved. Restart the server to apply."; msg.style.color="var(--ok)"; }
  else { const j = await r.json().catch(()=>({error:"error"})); msg.textContent = "✕ "+(j.error||"error"); msg.style.color="var(--danger)"; }
}

let CONSOLE_LOG = "";
function renderConsole(){
  return `<div class="card"><h2>Command console</h2>
    <p class="muted">Runs any slash command as Owner. e.g. <code>/ban Bob</code>, <code>/announce hi</code>, <code>/roomflags</code></p>
    <div id="console-out">${esc(CONSOLE_LOG)}</div>
    <div class="row" style="margin-top:8px"><input id="cmdIn" placeholder="/command args" style="flex:1" autofocus><button id="cmdRun">Run</button></div></div>`;
}
function appendConsole(t){ CONSOLE_LOG += t + "\n"; const el=document.getElementById("console-out"); if(el){el.textContent=CONSOLE_LOG; el.scrollTop=el.scrollHeight;} }

function wire(){
  document.querySelectorAll("[data-act]").forEach(b=>b.onclick=()=>{
    const n=b.dataset.n, a=b.dataset.act;
    if(a==="ban"&&!confirm("Ban "+n+"?"))return;
    run(`/${a} ${n}`);
  });
  document.querySelectorAll("[data-grant]").forEach(s=>s.onchange=()=>{ if(s.value) run(`/grant ${s.dataset.grant} ${s.value}`); });
  document.querySelectorAll("[data-flag]").forEach(b=>b.onclick=()=>run(`/${b.dataset.flag} ${b.dataset.on==="true"?"off":"on"}`));
  document.querySelectorAll("[data-remgreet]").forEach(b=>b.onclick=()=>run(`/remgreet ${b.dataset.remgreet}`));
  document.querySelectorAll("[data-remfilter]").forEach(b=>b.onclick=()=>run(`/remfilter ${b.dataset.remfilter}`));
  document.querySelectorAll("[data-runban]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/rangeunban ${a.dataset.runban}`);});
  document.querySelectorAll("[data-unasn]").forEach(a=>a.onclick=e=>{e.preventDefault();run(`/asnunban ${a.dataset.unasn}`);});
  const g=(id)=>document.getElementById(id);
  if(g("topicSet"))g("topicSet").onclick=()=>run(`/topic ${g("topicIn").value}`);
  if(g("statusSet"))g("statusSet").onclick=()=>run(`/status ${g("statusIn").value}`);
  if(g("clearBans"))g("clearBans").onclick=()=>{if(confirm("Clear ALL bans?"))run("/clearbans");};
  if(g("rbAdd"))g("rbAdd").onclick=()=>run(`/rangeban ${g("rbIn").value}`);
  if(g("abAdd"))g("abAdd").onclick=()=>run(`/asnban ${g("abIn").value}`);
  if(g("greetAdd"))g("greetAdd").onclick=()=>run(`/addgreet ${g("greetIn").value}`);
  if(g("greetToggle"))g("greetToggle").onclick=()=>run(`/greets ${STATE.greetsEnabled?"off":"on"}`);
  if(g("faddBtn"))g("faddBtn").onclick=()=>run(`/addfilter ${g("fpat").value} ${g("fact").value}`);
  if(g("cmdRun")){const runc=()=>{const l=g("cmdIn").value.trim(); if(l){run(l); g("cmdIn").value="";}}; g("cmdRun").onclick=runc; g("cmdIn").onkeydown=e=>{if(e.key==="Enter")runc();};}
  if(g("tomlEd")){ loadSettings(); g("tomlSave").onclick=saveSettings; g("tomlReload").onclick=loadSettings; }
}

document.querySelectorAll("#tabs button").forEach(b=>b.onclick=()=>{
  TAB=b.dataset.tab;
  document.querySelectorAll("#tabs button").forEach(x=>x.classList.remove("active"));
  b.classList.add("active"); render();
});
document.getElementById("refreshBtn").onclick=refresh;
document.getElementById("logoutBtn").onclick=logout;

async function login(){
  const pw=document.getElementById("pw").value;
  const r=await fetch("/admin/login",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({password:pw})});
  if(!r.ok){document.getElementById("loginErr").textContent="Invalid password."; return;}
  const j=await r.json(); TOKEN=j.token;
  sessionStorage.setItem("astra_token",TOKEN);
  document.getElementById("login").classList.add("hidden");
  document.getElementById("app").classList.remove("hidden");
  await refresh();
  if(!window._poll) window._poll=setInterval(()=>{ if(TAB!=="console"&&TAB!=="settings") refresh(); },5000);
}
function logout(){ TOKEN=null; sessionStorage.removeItem("astra_token"); location.reload(); }
document.getElementById("loginBtn").onclick=login;
document.getElementById("pw").onkeydown=e=>{if(e.key==="Enter")login();};

// Auto-login si hay token guardado.
(async()=>{ const t=sessionStorage.getItem("astra_token"); if(t){TOKEN=t; const r=await api("/admin/state"); if(r.ok){
  document.getElementById("login").classList.add("hidden"); document.getElementById("app").classList.remove("hidden");
  STATE=await r.json(); render(); refresh(); if(!window._poll) window._poll=setInterval(()=>{if(TAB!=="console"&&TAB!=="settings")refresh();},5000);
} else { TOKEN=null; sessionStorage.removeItem("astra_token"); } }})();
</script>
</body>
</html>
"####;
