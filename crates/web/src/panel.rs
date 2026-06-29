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
const ws = new WebSocket("ws://" + location.host + "/");
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
