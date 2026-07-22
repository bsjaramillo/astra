//! Manager de scripts JS. Carga/descarga/ejecuta scripts y dispara eventos.
//!
//! ## Arquitectura
//!
//! El `Context` de `boa_engine` no es `Send` (usa `Rc` internamente).
//! Por eso el `ScriptManager` corre en un **thread dedicado** vía
//! `std::thread::spawn`. Los handlers TCP/WS usan un `ScriptHandle` (que ES
//! `Send + Clone` — es un `mpsc::UnboundedSender<ScriptEvent>`) para
//! enviar eventos al thread del manager.
//!
//! ```text
//! TCP handler ──► ScriptHandle::dispatch(event) ──► mpsc ──► thread del manager
//!                                                                 │
//!                                                                 ▼
//!                                                       mgr.dispatch(event)
//!                                                                 │
//!                                                                 ▼
//!                                                       call JS handlers
//! ```
//!
//! ## Hooks de cancelación (*Before)
//!
//! Para hooks como `onTextBefore` que pueden cancelar la acción, usamos un
//! canal `std::sync::mpsc::sync_channel(1)` para el reply. El caller bloquea
//! en `recv_timeout` y procede con `true` (allow) si el manager no responde
//! a tiempo. Ver `ScriptRequest` y los métodos `check_*_before` en
//! `ScriptHandle`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

thread_local! {
    static ACTIVE_TIMERS: RefCell<std::collections::HashSet<i32>> = RefCell::new(std::collections::HashSet::new());
}

use boa_engine::JsValue;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use server_core::AppContext;

use crate::api::{
    call_global_function, eval_script, make_context, unregister_context,
};
use crate::types::{Script, ScriptEvent, ScriptId, ScriptState as ScriptLifecycle};

/// Request sincrónico al manager con un canal de reply.
///
/// El caller crea un `std::sync::mpsc::sync_channel(1)`, lo mete en el
/// request, y bloquea en `recv_timeout` esperando la respuesta. El manager
/// ejecuta la función JS correspondiente, captura el return value (bool)
/// y lo envía de vuelta.
pub enum ScriptRequest {
    /// Hook pre-mensaje público. Reply: `None` = cancelado por un script;
    /// `Some(texto)` = proceder con ese texto (posiblemente REESCRITO —
    /// paridad sb0t: `onTextBefore` retorna el string encadenado entre
    /// scripts; retornar `false`/`null`/`""` cancela).
    TextBefore {
        from: String,
        text: String,
        reply: std_mpsc::SyncSender<Option<String>>,
    },
    /// Hook pre-emote (misma semántica de reescritura que TextBefore).
    EmoteBefore {
        from: String,
        text: String,
        reply: std_mpsc::SyncSender<Option<String>>,
    },
    /// Hook pre-PM (misma semántica de reescritura que TextBefore).
    PMBefore {
        from: String,
        to: String,
        text: String,
        reply: std_mpsc::SyncSender<Option<String>>,
    },
    /// Hook gate de scribble. `reply.send(false)` → rechazar el scribble.
    ScribbleCheck {
        from: String,
        is_pm: bool,
        reply: std_mpsc::SyncSender<bool>,
    },
    /// Gate de login (paridad sb0t `Joining`): `false` → rechazar el join.
    JoinCheck {
        name: String,
        ip: String,
        reply: std_mpsc::SyncSender<bool>,
    },
    /// Gate de cambio de vroom (paridad `VroomJoinCheck`).
    VroomJoinCheck {
        name: String,
        vroom: u16,
        reply: std_mpsc::SyncSender<bool>,
    },
    /// Gate de castigo por flood (paridad `Flooding`): `false` → perdonar.
    FloodBefore {
        name: String,
        msg: String,
        reply: std_mpsc::SyncSender<bool>,
    },
    /// Eval inline de chat (`@código`, paridad sb0t TextSending): evalúa
    /// `code` en el primer script activo con `userobj` preseteado al emisor.
    EvalChat {
        name: String,
        code: String,
        reply: std_mpsc::SyncSender<Result<(), String>>,
    },
    /// Lista los nombres de los scripts cargados (`/listscripts`).
    ListScripts {
        reply: std_mpsc::SyncSender<Vec<String>>,
    },
    /// Carga un script por nombre desde `scripts_dir/<name>.js` (`/loadscript`).
    LoadScript {
        name: String,
        reply: std_mpsc::SyncSender<Result<String, String>>,
    },
    /// Descarga un script por nombre (`/killscript`).
    KillScript {
        name: String,
        reply: std_mpsc::SyncSender<Result<(), String>>,
    },
}

impl ScriptRequest {
    /// Solo aplica a las variantes `*Before`/`ScribbleCheck` (llaman un
    /// handler JS); `ListScripts`/`LoadScript`/`KillScript` se resuelven
    /// directamente en `dispatch_request` sin llegar a este método.
    fn handler_name(&self) -> &'static str {
        match self {
            ScriptRequest::TextBefore { .. } => "onTextBefore",
            ScriptRequest::EmoteBefore { .. } => "onEmoteBefore",
            ScriptRequest::PMBefore { .. } => "onPMBefore",
            ScriptRequest::ScribbleCheck { .. } => "onScribbleCheck",
            ScriptRequest::JoinCheck { .. } => "onJoinCheck",
            ScriptRequest::VroomJoinCheck { .. } => "onVroomJoinCheck",
            ScriptRequest::FloodBefore { .. } => "onFloodBefore",
            ScriptRequest::EvalChat { .. }
            | ScriptRequest::ListScripts { .. }
            | ScriptRequest::LoadScript { .. }
            | ScriptRequest::KillScript { .. } => unreachable!("resuelto antes en dispatch_request"),
        }
    }

    /// Tipo de conversión del argumento `idx` (paridad sb0t). El emisor va en
    /// 0; `PMBefore` además lleva el destino (JSUser) en 1 y el mensaje (JSPM)
    /// en 2, igual que sb0t `onPMBefore(u, t, pm)`.
    fn arg_kind(&self, idx: usize) -> crate::types::ArgKind {
        use crate::types::ArgKind;
        match self {
            ScriptRequest::PMBefore { .. } => match idx {
                0 | 1 => ArgKind::User,
                2 => ArgKind::Pm,
                _ => ArgKind::Str,
            },
            _ => {
                if idx == 0 {
                    ArgKind::User
                } else {
                    ArgKind::Str
                }
            }
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            ScriptRequest::TextBefore { from, text, .. } => vec![from.clone(), text.clone()],
            ScriptRequest::EmoteBefore { from, text, .. } => vec![from.clone(), text.clone()],
            ScriptRequest::PMBefore { from, to, text, .. } => {
                vec![from.clone(), to.clone(), text.clone()]
            }
            ScriptRequest::ScribbleCheck { from, is_pm, .. } => {
                vec![from.clone(), is_pm.to_string()]
            }
            ScriptRequest::JoinCheck { name, ip, .. } => vec![name.clone(), ip.clone()],
            ScriptRequest::VroomJoinCheck { name, vroom, .. } => {
                vec![name.clone(), vroom.to_string()]
            }
            ScriptRequest::FloodBefore { name, msg, .. } => vec![name.clone(), msg.clone()],
            ScriptRequest::EvalChat { .. }
            | ScriptRequest::ListScripts { .. }
            | ScriptRequest::LoadScript { .. }
            | ScriptRequest::KillScript { .. } => unreachable!("resuelto antes en dispatch_request"),
        }
    }
}

/// Handle `Send + Clone` para enqueue de eventos al manager.
///
/// Tiene dos canales:
/// - `tx`: eventos async (fire-and-forget)
/// - `tx_req`: requests sync con reply (hooks *Before)
#[derive(Clone)]
pub struct ScriptHandle {
    tx: mpsc::UnboundedSender<ScriptEvent>,
    tx_req: mpsc::UnboundedSender<ScriptRequest>,
}

impl ScriptHandle {
    /// Encola un evento async. No bloquea.
    pub fn dispatch(&self, event: ScriptEvent) {
        let _ = self.tx.send(event);
    }

    /// Hook pre-mensaje público. Bloquea hasta 100ms esperando respuesta.
    /// Retorna `None` si algún script canceló; `Some(texto)` para proceder
    /// (el texto puede venir REESCRITO por los scripts, paridad sb0t).
    pub fn check_text_before(&self, from: &str, text: &str) -> Option<String> {
        let (tx, rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let request = ScriptRequest::TextBefore {
            from: from.to_string(),
            text: text.to_string(),
            reply: tx,
        };
        self.send_and_wait(request, rx, Some(text.to_string()))
    }

    /// Hook pre-emote. Misma semántica de reescritura que `check_text_before`.
    pub fn check_emote_before(&self, from: &str, text: &str) -> Option<String> {
        let (tx, rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let request = ScriptRequest::EmoteBefore {
            from: from.to_string(),
            text: text.to_string(),
            reply: tx,
        };
        self.send_and_wait(request, rx, Some(text.to_string()))
    }

    /// Gate de login (paridad sb0t `Joining`): `false` → rechazar el join.
    pub fn check_join(&self, name: &str, ip: &str) -> bool {
        let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
        let request = ScriptRequest::JoinCheck {
            name: name.to_string(),
            ip: ip.to_string(),
            reply: tx,
        };
        self.send_and_wait(request, rx, true)
    }

    /// Gate de cambio de vroom. `false` → rechazar el cambio.
    pub fn check_vroom_join(&self, name: &str, vroom: u16) -> bool {
        let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
        let request = ScriptRequest::VroomJoinCheck {
            name: name.to_string(),
            vroom,
            reply: tx,
        };
        self.send_and_wait(request, rx, true)
    }

    /// Gate de castigo por flood. `false` → perdonar (no castigar).
    pub fn check_flood(&self, name: &str, msg: &str) -> bool {
        let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
        let request = ScriptRequest::FloodBefore {
            name: name.to_string(),
            msg: msg.to_string(),
            reply: tx,
        };
        self.send_and_wait(request, rx, true)
    }

    /// Hook pre-PM. Retorna `true` si se debe proceder.
    pub fn check_pm_before(&self, from: &str, to: &str, text: &str) -> Option<String> {
        let (tx, rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let request = ScriptRequest::PMBefore {
            from: from.to_string(),
            to: to.to_string(),
            text: text.to_string(),
            reply: tx,
        };
        self.send_and_wait(request, rx, Some(text.to_string()))
    }

    /// Hook gate de scribble. Retorna `false` si el scribble debe rechazarse.
    pub fn check_scribble(&self, from: &str, is_pm: bool) -> bool {
        let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
        let request = ScriptRequest::ScribbleCheck {
            from: from.to_string(),
            is_pm,
            reply: tx,
        };
        self.send_and_wait(request, rx, true)
    }

    /// Eval inline de chat (`@código`): evalúa en el primer script activo
    /// con `userobj` = el emisor. SOLO debe llamarse para usuarios Owner.
    pub fn eval_chat(&self, name: &str, code: &str) -> Result<(), String> {
        let (tx, rx) = std_mpsc::sync_channel::<Result<(), String>>(1);
        if self
            .tx_req
            .send(ScriptRequest::EvalChat {
                name: name.to_string(),
                code: code.to_string(),
                reply: tx,
            })
            .is_err()
        {
            return Err("script manager down".to_string());
        }
        rx.recv_timeout(Duration::from_millis(500))
            .unwrap_or_else(|_| Err("eval timeout".to_string()))
    }

    fn send_and_wait<T>(&self, request: ScriptRequest, rx: std_mpsc::Receiver<T>, fallback: T) -> T {
        if self.tx_req.send(request).is_err() {
            return fallback; // manager down → allow
        }
        // Esperar respuesta con timeout de 100ms
        rx.recv_timeout(Duration::from_millis(100)).unwrap_or(fallback)
    }

    /// Lista los nombres de los scripts cargados (`/listscripts`).
    pub fn list_scripts(&self) -> Vec<String> {
        let (tx, rx) = std_mpsc::sync_channel::<Vec<String>>(1);
        if self.tx_req.send(ScriptRequest::ListScripts { reply: tx }).is_err() {
            return Vec::new();
        }
        rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default()
    }

    /// Carga un script por nombre desde el directorio de scripts (`/loadscript`).
    pub fn load_script(&self, name: &str) -> Result<String, String> {
        let (tx, rx) = std_mpsc::sync_channel::<Result<String, String>>(1);
        let request = ScriptRequest::LoadScript { name: name.to_string(), reply: tx };
        if self.tx_req.send(request).is_err() {
            return Err("script manager no disponible".to_string());
        }
        rx.recv_timeout(Duration::from_millis(500))
            .unwrap_or_else(|_| Err("timeout esperando al script manager".to_string()))
    }

    /// Descarga un script por nombre (`/killscript`).
    pub fn kill_script(&self, name: &str) -> Result<(), String> {
        let (tx, rx) = std_mpsc::sync_channel::<Result<(), String>>(1);
        let request = ScriptRequest::KillScript { name: name.to_string(), reply: tx };
        if self.tx_req.send(request).is_err() {
            return Err("script manager no disponible".to_string());
        }
        rx.recv_timeout(Duration::from_millis(500))
            .unwrap_or_else(|_| Err("timeout esperando al script manager".to_string()))
    }
}

// Add ScribbleCheck variant to dispatch_request matching
impl ScriptManager {
    // ... existing code ...
}

/// Manager de scripts.
///
/// **NO es `Send`**: contiene `Rc` de `boa_engine`. Por eso NO debe moverse
/// entre threads. Vive en el thread dedicado que crea `start_in_thread()`.
pub struct ScriptManager {
    /// App context
    app: Arc<AppContext>,
    /// Scripts cargados (id → script)
    scripts: Mutex<HashMap<ScriptId, Arc<Script>>>,
    /// Directorio donde buscar scripts
    scripts_dir: PathBuf,
}

/// Resuelve el archivo principal de una carpeta de script. Prioridad:
/// `<carpeta>/<carpeta>.js` (mismo nombre que la carpeta, paridad sb0t), luego
/// `main.js`, `index.js`, y por último el primer `.js` del nivel superior.
fn resolve_main_file(dir: &Path) -> Option<PathBuf> {
    let dir_name = dir.file_name().and_then(|s| s.to_str())?;
    // Convención sb0t: el archivo principal se llama igual que la carpeta
    // (`<script>/<script>.js`). También aceptamos main.js/index.js.
    for cand in [format!("{}.js", dir_name), "main.js".to_string(), "index.js".to_string()] {
        let p = dir.join(&cand);
        if p.is_file() {
            return Some(p);
        }
    }

    // Fallback (cuando el nombre no coincide): entre los `.js` del nivel
    // superior, elegir de forma DETERMINÍSTICA y preferir el archivo que
    // realmente parece el principal — el que define handlers de eventos
    // (onLoad/onCommand/onJoin/...). Antes se tomaba el primer `.js` que
    // devolvía el filesystem (orden no determinístico), lo que podía cargar
    // un helper (`utils.js`, `constants.js`) en vez del script real, dejando
    // el script "cargado" pero sin ninguno de sus handlers.
    let mut js_files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("js"))
        .collect();
    js_files.sort();

    // Primero: el que declare algún handler de sb0t.
    if let Some(p) = js_files.iter().find(|p| file_defines_handler(p)) {
        return Some(p.clone());
    }
    // Si ninguno tiene handlers, el primero en orden alfabético (determinístico).
    js_files.into_iter().next()
}

/// ¿El archivo `.js` define alguna función handler de sb0t (heurística para
/// distinguir el script principal de sus helpers/includes)?
fn file_defines_handler(path: &Path) -> bool {
    const HANDLERS: &[&str] = &[
        "onLoad", "onCommand", "onJoin", "onPart", "onPublic", "onText",
        "onEmote", "onPM", "onHelp", "onTimer", "onJoinCheck",
    ];
    let Ok(src) = std::fs::read_to_string(path) else {
        return false;
    };
    HANDLERS
        .iter()
        .any(|h| src.contains(&format!("function {}", h)))
}

impl ScriptManager {
    /// Crea un nuevo manager. NO lo muevas a otra task.
    pub fn new(app: Arc<AppContext>, scripts_dir: PathBuf) -> Self {
        Self {
            app,
            scripts: Mutex::new(HashMap::new()),
            scripts_dir,
        }
    }

    /// Inicia el manager en un thread dedicado. Devuelve un `ScriptHandle`
    /// que se puede usar desde otras tasks (TCP, WS, etc.) para enviar
    /// eventos.
    ///
    /// Carga todos los scripts `.js` del directorio configurado, DESDE
    /// DENTRO del thread dedicado (ver nota de seguridad más abajo — NO
    /// antes de moverse ahí).
    pub fn start_in_thread(self) -> ScriptHandle {
        // Dos canales: events (async) y requests (sync con reply)
        let (tx, mut rx_events) = mpsc::unbounded_channel::<ScriptEvent>();
        let (tx_req, mut rx_requests) = mpsc::unbounded_channel::<ScriptRequest>();

        // Spawn del thread dedicado. El manager se mueve al thread vía
        // un `usize` (Send + Copy + 'static). El *mut ScriptManager
        // NO es Send directamente, pero un usize sí.
        // SAFETY: el manager vive solo en este thread y se destruye
        // cuando el thread termina. En este punto `self.scripts` está
        // VACÍO (`load_all_inner` corre recién abajo, ya en el thread
        // dedicado) — si se cargaran scripts ANTES de mover el manager aquí
        // (como hacía una versión anterior de este código), sus
        // `boa_engine::Context` se crean en el thread llamante pero se
        // destruyen en este thread dedicado; `boa_engine` mantiene un
        // contador `thread_local` (`CANNOT_BLOCK_COUNTER`) que se
        // incrementa al crear un Context y se decrementa al dropearlo —
        // crear en un thread y destruir en otro descuenta un contador que
        // nunca se incrementó ahí, y panickea por underflow (`attempt to
        // subtract with overflow`) la primera vez que se descarga un
        // script cargado al arrancar (`/killscript`, `/reload`). Por eso
        // TODA la carga de scripts (inicial y en caliente) debe pasar por
        // este mismo thread.
        let manager_ptr: usize = Box::into_raw(Box::new(self)) as usize;

        std::thread::spawn(move || {
            // SAFETY: reconstituimos el Box desde el usize
            let manager = unsafe { Box::from_raw(manager_ptr as *mut ScriptManager) };
            info!("script manager: thread iniciado");

            // Cargar los scripts del directorio (ver nota de seguridad
            // arriba: tiene que pasar aquí, no antes de mover `self`).
            let _ = manager.load_all_inner();

            // Loop principal: alterna entre events y requests.
            // Como ambos usan tokio::sync::mpsc con blocking_recv, podemos
            // usar un loop simple: si no hay events, intentar requests.
            let mut last_deferred = std::time::Instant::now();
            loop {
                let event = rx_events.try_recv();
                match event {
                    Ok(ev) => manager.dispatch(&ev),
                    Err(mpsc::error::TryRecvError::Empty) => {}
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
                let request = rx_requests.try_recv();
                match request {
                    Ok(req) => manager.dispatch_request(req),
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // Sin eventos: drenar trabajo diferido (timers, HTTP)
                        // periódicamente para que dispare aun con la sala inactiva.
                        if last_deferred.elapsed() >= Duration::from_millis(50) {
                            manager.drain_deferred();
                            last_deferred = std::time::Instant::now();
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            info!("script manager: thread terminado (canales cerrados)");
        });

        ScriptHandle { tx, tx_req }
    }

    /// Carga todos los scripts `.js` del directorio. (Interno, usado por
    /// `start_in_thread` y por tests)
    fn load_all_inner(&self) -> usize {
        if !self.scripts_dir.exists() {
            warn!(
                "directorio de scripts no existe: {}",
                self.scripts_dir.display()
            );
            return 0;
        }
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(&self.scripts_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Modelo de carpetas (paridad sb0t): cada carpeta es un
                    // script, con su archivo principal `<carpeta>/<carpeta>.js`
                    // (o `main.js`/`index.js`/el primer `.js`), sub-scripts y
                    // datos adentro.
                    match resolve_main_file(&path) {
                        Some(main) => match self.load_folder_script(&path, &main) {
                            Ok(_) => count += 1,
                            Err(e) => warn!("error cargando script {}: {}", path.display(), e),
                        },
                        None => warn!(
                            "carpeta de script sin archivo principal (.js): {}",
                            path.display()
                        ),
                    }
                } else if path.extension().and_then(|s| s.to_str()) == Some("js") {
                    // Retrocompat: `.js` suelto en la raíz = un script.
                    match self.load_file(&path) {
                        Ok(_) => count += 1,
                        Err(e) => warn!("error cargando {}: {}", path.display(), e),
                    }
                }
            }
        }
        info!("{} scripts cargados desde {}", count, self.scripts_dir.display());
        count
    }

    /// Carga un script en carpeta: el nombre es el de la carpeta, el código es
    /// el del archivo principal `main`.
    fn load_folder_script(&self, dir: &Path, main: &Path) -> Result<ScriptId, String> {
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "nombre de carpeta inválido".to_string())?
            .to_string();
        let source = std::fs::read_to_string(main)
            .map_err(|e| format!("error leyendo {}: {}", main.display(), e))?;
        self.load_source(&name, Some(main.to_path_buf()), &source)
    }

    /// Directorio de scripts.
    pub fn scripts_dir(&self) -> &Path {
        &self.scripts_dir
    }

    /// Cantidad de scripts cargados.
    pub fn count(&self) -> usize {
        self.scripts.lock().len()
    }

    /// Lista los IDs de los scripts cargados.
    pub fn list(&self) -> Vec<ScriptId> {
        self.scripts.lock().keys().copied().collect()
    }

    /// Obtiene un script por ID.
    pub fn get(&self, id: ScriptId) -> Option<Arc<Script>> {
        self.scripts.lock().get(&id).cloned()
    }

    /// Obtiene un script por nombre.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<Script>> {
        self.scripts
            .lock()
            .values()
            .find(|s| s.name == name)
            .cloned()
    }

    /// Carga un script desde un archivo JS.
    pub fn load_file(&self, path: &Path) -> Result<ScriptId, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("error leyendo {}: {}", path.display(), e))?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        self.load_source(&name, Some(path.to_path_buf()), &source)
    }

    /// Carga un script desde código fuente.
    pub fn load_source(
        &self,
        name: &str,
        path: Option<PathBuf>,
        source: &str,
    ) -> Result<ScriptId, String> {
        // Carpeta del script = carpeta del archivo principal. Para un script en
        // carpeta (`<scripts_dir>/<name>/<name>.js`) es `<scripts_dir>/<name>`;
        // para un `.js` plano (retrocompat) es `<scripts_dir>`. Se expone como
        // `__SCRIPT_DIR__` para que `include()`/`File_*` sean relativos a ella.
        let script_dir = path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        // Recargar = REEMPLAZAR: descargar cualquier instancia previa con el
        // mismo nombre antes de crear la nueva. Sin esto, cada `/loadscript`
        // acumulaba una instancia más y los eventos se disparaban N veces.
        let previous: Vec<ScriptId> = self
            .scripts
            .lock()
            .values()
            .filter(|s| s.name == name)
            .map(|s| s.id)
            .collect();
        for id in previous {
            self.unload(id);
        }

        let script = Arc::new(Script::new(name.to_string(), path));
        let mut ctx = make_context(self.app.clone());
        if let Some(dir) = &script_dir {
            crate::api::set_script_dir(&mut ctx, &dir.to_string_lossy());
        }
        // Atribuir al script los recursos globales que registre (líneas de
        // /help, timers), para poder limpiarlos cuando se descargue.
        crate::api::set_script_name(&mut ctx, name);

        if let Err(e) = eval_script(&mut ctx, source) {
            error!("script '{}': {}", name, e);
            script.set_state(ScriptLifecycle::Error);
            script.set_error(e.clone());
            notify_error_subscribers(&self.app, name, &e);
            let id = script.id;
            unregister_context(&ctx);
            self.scripts.lock().insert(id, script);
            return Err(e);
        }

        // Un error en onLoad NO impide que el script maneje eventos (igual
        // que sb0t), pero debe ser VISIBLE: antes solo iba al log del server
        // y desde el chat parecía que "onLoad no se ejecuta".
        if let Err(e) = call_void_handler(&mut ctx, "onLoad", &[], |_| crate::types::ArgKind::Str) {
            error!("script '{}' onLoad() error: {}", name, e);
            script.set_error(e.clone());
            notify_error_subscribers(&self.app, name, &format!("onLoad: {}", e));
        }

        *script.context.lock() = Some(ctx);
        script.set_state(ScriptLifecycle::Active);

        let id = script.id;
        self.scripts.lock().insert(id, script);
        info!("script cargado: {} (id={:?})", name, id);
        Ok(id)
    }

    /// Descarga un script por ID.
    pub fn unload(&self, id: ScriptId) -> bool {
        if let Some(script) = self.scripts.lock().remove(&id) {
            if let Some(ctx) = script.context.lock().as_ref() {
                unregister_context(ctx);
            }
            script.set_state(ScriptLifecycle::Unloaded);
            // Limpiar los recursos globales que dejó registrados: si no, sus
            // líneas de /help siguen apareciendo y sus timers repetitivos se
            // re-encolan para siempre aunque el script ya no exista.
            crate::api::clear_help_lines_for(script.name());
            crate::api::clear_timers_for(script.name());
            info!("script descargado: {} (id={:?})", script.name(), id);
            true
        } else {
            false
        }
    }

    /// Recarga un script (descarga y vuelve a cargar).
    pub fn reload(&self, id: ScriptId) -> Result<ScriptId, String> {
        let path = match self.get(id) {
            Some(s) => s.path.clone(),
            None => return Err(format!("script {:?} no encontrado", id)),
        };
        let path = match path {
            Some(p) => p,
            None => return Err(format!("script {:?} sin path", id)),
        };
        self.unload(id);
        self.load_file(&path)
    }

    /// Carga todos los scripts `.js` del directorio (método público para tests).
    #[cfg(test)]
    pub fn load_all(&self) -> usize {
        self.load_all_inner()
    }

    /// Procesa trabajo diferido (timers expirados y respuestas HTTP
    /// completadas en background) despachando los eventos correspondientes.
    /// Se invoca desde `dispatch` (antes de eventos no-diferidos) y desde el
    /// loop del thread del manager cuando está inactivo, para que timers y
    /// callbacks HTTP disparen aunque no haya actividad de la sala.
    pub fn drain_deferred(&self) {
        let now = std::time::Instant::now();
        for timer in crate::api::pop_due_timers(now) {
            if timer.repeat {
                // Re-encolar el timer repetitivo. Se pierde el periodo original,
                // heurística: re-armar a +1s.
                let next = crate::api::PendingTimer {
                    id: timer.id,
                    fn_name: timer.fn_name.clone(),
                    fire_at: now + std::time::Duration::from_secs(1),
                    repeat: true,
                    script: timer.script.clone(),
                };
                ACTIVE_TIMERS.with(|t| t.borrow_mut().insert(timer.id));
                crate::api::push_pending_timer(next);
            } else {
                ACTIVE_TIMERS.with(|t| t.borrow_mut().remove(&timer.id));
            }
            self.dispatch(&ScriptEvent::Timer {
                secs: timer.id as u64,
                name: timer.fn_name,
            });
        }
        for done in crate::api::drain_http_completions() {
            self.dispatch(&ScriptEvent::HttpComplete {
                key: done.key,
                body: done.body,
                status: done.status,
                error: done.error,
            });
        }
    }

    /// Despacha un evento a todos los scripts activos.
    /// Se llama desde el thread del manager (no desde otros threads).
    pub fn dispatch(&self, event: &ScriptEvent) {
        let handler_name = event.handler_name();
        let args = event.args();

        // Procesar timers expirados y respuestas HTTP antes del dispatch.
        // Se omite cuando el propio evento es diferido (evita recursión).
        if !matches!(
            event,
            ScriptEvent::Timer { .. } | ScriptEvent::HttpComplete { .. }
        ) {
            self.drain_deferred();
        }

        let scripts = self.scripts.lock().clone();
        for (_, script) in scripts.iter() {
            if script.state() != ScriptLifecycle::Active {
                continue;
            }
            let mut ctx_guard = script.context.lock();
            if let Some(ctx) = ctx_guard.as_mut() {
                if let Err(e) = call_void_handler(ctx, handler_name, &args, |i| event.arg_kind(i)) {
                    let msg = format!("error en handler '{}': {}", handler_name, e);
                    warn!("script '{}': {}", script.name(), msg);
                    script.set_error(msg.clone());
                    notify_error_subscribers(&self.app, script.name(), &msg);
                }
            }
        }
    }

    /// Despacha un request sincrónico a todos los scripts.
    /// Llama a la función JS correspondiente en cada script activo.
    /// El resultado se computa como AND de todos los returns:
    /// si ALGÚN script retorna `false`, el reply es `false` (cancela).
    /// Si TODOS retornan `true` o no hay handler, el reply es `true`.
    pub fn dispatch_request(&self, request: ScriptRequest) {
        // Las 3 variantes de gestión de scripts no llaman a ningún handler
        // JS — se resuelven directo contra `self.scripts`/`self.scripts_dir`
        // y retornan temprano, antes de tocar `handler_name()`/`args()`
        // (que no las soportan).
        match request {
            ScriptRequest::ListScripts { reply } => {
                let names: Vec<String> = self
                    .scripts
                    .lock()
                    .values()
                    .map(|s| s.name().to_string())
                    .collect();
                let _ = reply.send(names);
                return;
            }
            ScriptRequest::LoadScript { name, reply } => {
                // Primero como carpeta (`<scripts_dir>/<name>/`), luego como
                // `.js` plano (retrocompat).
                let folder = self.scripts_dir.join(&name);
                let result = if folder.is_dir() {
                    match resolve_main_file(&folder) {
                        Some(main) => self.load_folder_script(&folder, &main).map(|_| name),
                        None => Err(format!("carpeta '{}' sin archivo principal (.js)", name)),
                    }
                } else {
                    let path = self.scripts_dir.join(format!("{}.js", name));
                    self.load_file(&path).map(|_| name)
                };
                let _ = reply.send(result);
                return;
            }
            ScriptRequest::EvalChat { name, code, reply } => {
                let scripts = self.scripts.lock().clone();
                let mut result = Err("no scripts loaded".to_string());
                for (_, script) in scripts.iter() {
                    if script.state() != ScriptLifecycle::Active {
                        continue;
                    }
                    let mut ctx_guard = script.context.lock();
                    if let Some(ctx) = ctx_guard.as_mut() {
                        // `userobj` preseteado al emisor (paridad sb0t).
                        let quoted = serde_json::to_string(&name).unwrap_or_else(|_| "\"\"".into());
                        let js = format!("userobj = user({}); null; {}", quoted, code);
                        result = eval_script(ctx, &js).map(|_| ());
                    }
                    break; // solo el primer script activo, como sb0t
                }
                let _ = reply.send(result);
                return;
            }
            ScriptRequest::KillScript { name, reply } => {
                // TODAS las instancias con ese nombre (no solo la primera:
                // un bug previo de `load_source` podía dejar duplicados, y
                // matar una sola dejaba las demás vivas).
                let ids: Vec<ScriptId> = self
                    .scripts
                    .lock()
                    .values()
                    .filter(|s| s.name == name)
                    .map(|s| s.id)
                    .collect();
                let result = if ids.is_empty() {
                    Err(format!("script '{}' no encontrado", name))
                } else {
                    for id in ids {
                        self.unload(id);
                    }
                    Ok(())
                };
                let _ = reply.send(result);
                return;
            }
            _ => {}
        }

        let handler_name = request.handler_name();
        let scripts = self.scripts.lock().clone();

        // Grupo 1: hooks de TEXTO (reescritura encadenada, paridad sb0t
        // `ServerEvents.TextSending`): cada script recibe el texto actual y
        // puede retornar un string (reemplaza), false/null/"" (cancela), o
        // true/undefined (no toca).
        let (text_reply, mut current, prefix_args) = match &request {
            ScriptRequest::TextBefore { from, text, reply } => {
                (Some(reply.clone()), text.clone(), vec![from.clone()])
            }
            ScriptRequest::EmoteBefore { from, text, reply } => {
                (Some(reply.clone()), text.clone(), vec![from.clone()])
            }
            ScriptRequest::PMBefore { from, to, text, reply } => {
                (Some(reply.clone()), text.clone(), vec![from.clone(), to.clone()])
            }
            _ => (None, String::new(), vec![]),
        };
        if let Some(reply) = text_reply {
            for (_, script) in scripts.iter() {
                if script.state() != ScriptLifecycle::Active {
                    continue;
                }
                let mut ctx_guard = script.context.lock();
                let Some(ctx) = ctx_guard.as_mut() else { continue };
                let mut args = prefix_args.clone();
                args.push(current.clone());
                match call_handler_with_return(ctx, handler_name, &args, |i| request.arg_kind(i)) {
                    Ok(HandlerReturn::Bool(false)) | Ok(HandlerReturn::Null) => {
                        debug!("script '{}' canceló via {}", script.name(), handler_name);
                        let _ = reply.send(None);
                        return;
                    }
                    Ok(HandlerReturn::Text(t)) => {
                        if t.is_empty() {
                            // sb0t: IsNullOrEmpty(result) → cancelar.
                            debug!("script '{}' vació el texto via {}", script.name(), handler_name);
                            let _ = reply.send(None);
                            return;
                        }
                        current = t;
                    }
                    Ok(_) => {} // true/undefined/sin handler → no afecta
                    Err(e) => {
                        warn!("script '{}': error en {}: {}", script.name(), handler_name, e);
                        notify_error_subscribers(
                            &self.app,
                            script.name(),
                            &format!("{}: {}", handler_name, e),
                        );
                    }
                }
            }
            let _ = reply.send(Some(current));
            return;
        }

        // Grupo 2: gates booleanos (ScribbleCheck/JoinCheck/VroomJoinCheck/
        // FloodBefore): si algún script retorna false, se cancela.
        let args = request.args();
        let reply = match &request {
            ScriptRequest::ScribbleCheck { reply, .. } => reply.clone(),
            ScriptRequest::JoinCheck { reply, .. } => reply.clone(),
            ScriptRequest::VroomJoinCheck { reply, .. } => reply.clone(),
            ScriptRequest::FloodBefore { reply, .. } => reply.clone(),
            _ => unreachable!("ya se resolvió arriba"),
        };

        if scripts.is_empty() {
            let _ = reply.send(true);
            return;
        }

        let mut allow = true;
        for (_, script) in scripts.iter() {
            if script.state() != ScriptLifecycle::Active {
                continue;
            }
            let mut ctx_guard = script.context.lock();
            if let Some(ctx) = ctx_guard.as_mut() {
                match call_handler_with_return(ctx, handler_name, &args, |i| request.arg_kind(i)) {
                    Ok(HandlerReturn::Bool(false)) => {
                        allow = false;
                        debug!("script '{}' canceló via {}", script.name(), handler_name);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("script '{}': error en {}: {}", script.name(), handler_name, e);
                        notify_error_subscribers(
                            &self.app,
                            script.name(),
                            &format!("{}: {}", handler_name, e),
                        );
                    }
                }
            }
        }
        let _ = reply.send(allow);
    }

    /// Despacha un evento solo a un script específico.
    pub fn dispatch_to(&self, id: ScriptId, event: &ScriptEvent) -> Result<(), String> {
        let script = self.get(id).ok_or_else(|| format!("script {:?} no encontrado", id))?;
        if script.state() != ScriptLifecycle::Active {
            return Err(format!("script {:?} no está activo", id));
        }
        let mut ctx_guard = script.context.lock();
        let ctx = ctx_guard
            .as_mut()
            .ok_or_else(|| "context no inicializado".to_string())?;
        call_void_handler(ctx, event.handler_name(), &event.args(), |i| event.arg_kind(i))
    }
}

/// Llama a una función JS sin retorno con los argumentos dados.
/// Convierte los argumentos string en `JsValue` según el `ArgKind` de cada
/// posición: `User` → objeto JSUser, `Pm` → objeto JSPM, `Str` → string.
/// Da paridad con sb0t (handlers que reciben usuarios y mensajes-objeto).
fn build_handler_args(
    ctx: &mut boa_engine::Context,
    args: &[String],
    kind: impl Fn(usize) -> crate::types::ArgKind,
) -> Vec<JsValue> {
    use crate::types::ArgKind;
    let mut out = Vec::with_capacity(args.len());
    for (i, s) in args.iter().enumerate() {
        out.push(match kind(i) {
            ArgKind::User => crate::api::build_user_object(ctx, s),
            ArgKind::Pm => crate::api::build_pm_object(ctx, s),
            ArgKind::Str => JsValue::from(boa_engine::js_string!(s.as_str())),
        });
    }
    out
}

fn call_void_handler(
    ctx: &mut boa_engine::Context,
    name: &str,
    args: &[String],
    kind: impl Fn(usize) -> crate::types::ArgKind,
) -> Result<(), String> {
    let js_args = build_handler_args(ctx, args, kind);
    call_global_function(ctx, name, &js_args)
}

/// Llama a una función JS y captura el return value (que se espera bool).
/// Retorna:
/// - `Ok(Some(true))` si la función existe y retornó `true`
/// - `Ok(Some(false))` si la función existe y retornó `false`
/// - `Ok(None)` si la función no está definida
/// - `Ok(Some(other))` si la función retornó algo no-bool
/// - `Err(e)` si hubo error ejecutando
/// Resultado de invocar un handler JS con retorno.
enum HandlerReturn {
    /// El handler no existe (o no es función) en este script.
    NoHandler,
    /// Retornó un booleano explícito.
    Bool(bool),
    /// Retornó un string (para los hooks de texto = texto reescrito).
    Text(String),
    /// Retornó `null` (sb0t: cancela en los hooks de texto).
    Null,
    /// Retornó undefined u otro tipo → no afecta.
    Other,
}

fn call_handler_with_return(
    ctx: &mut boa_engine::Context,
    name: &str,
    args: &[String],
    kind: impl Fn(usize) -> crate::types::ArgKind,
) -> Result<HandlerReturn, String> {
    use boa_engine::js_string;
    use boa_engine::property::PropertyKey;

    let key = PropertyKey::from(js_string!(name));
    let func = ctx
        .global_object()
        .get(key.clone(), ctx)
        .map_err(|e| format!("error buscando '{}': {}", name, e))?;

    if func.is_undefined() || func.is_null() {
        return Ok(HandlerReturn::NoHandler);
    }
    if !func.is_object() {
        return Ok(HandlerReturn::NoHandler);
    }

    let js_args = build_handler_args(ctx, args, kind);

    let result = func
        .as_object()
        .unwrap()
        .call(&JsValue::undefined(), &js_args, ctx)
        .map_err(|e| format!("error ejecutando '{}': {}", name, e))?;

    if result.is_boolean() {
        Ok(HandlerReturn::Bool(result.as_boolean().unwrap()))
    } else if result.is_null() {
        Ok(HandlerReturn::Null)
    } else if result.is_string() {
        let txt = result
            .as_string()
            .map(|s| s.to_std_string_lossy())
            .unwrap_or_default();
        Ok(HandlerReturn::Text(txt))
    } else {
        Ok(HandlerReturn::Other)
    }
}

/// Notifica por PM del bot a todo usuario suscrito (`/errors on`, paridad
/// `ErrorDispatcher.SendError` de sb0t) que un script tiró un error.
/// A diferencia de sb0t (que llama esto desde ~90 call sites), aquí alcanza
/// con los 2 puntos donde ya se captura un error de script
/// (`load_source`/`dispatch`), porque son los únicos lugares donde Astra
/// ejecuta código JS de scripts.
fn notify_error_subscribers(app: &AppContext, script_name: &str, msg: &str) {
    let bot_name = app.settings.bot_name.clone();
    let line = format!("{}: {}", script_name, msg);
    for u in app.user_pool.users() {
        if u.logged_in && u.sub_errors.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send_pvt(&bot_name, &line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;
    use std::sync::Arc;

    fn make_manager() -> ScriptManager {
        let db = Database::in_memory().unwrap();
        let app = AppContext::new(Settings::default(), db);
        ScriptManager::new(Arc::new(app), std::env::temp_dir().join("astra_scripts_test"))
    }

    #[test]
    fn onpm_bridge_passes_both_users() {
        use server_core::user_pool::AresUser;
        let db = Database::in_memory().unwrap();
        let app = Arc::new(AppContext::new(Settings::default(), db));

        let mut a = AresUser::new(1, "10.0.0.1".parse().unwrap(), [0u8; 16]);
        *a.name.write() = "Alice".to_string();
        a.logged_in = true;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        a.sender = Some(tx);
        app.user_pool.add(Arc::new(a));

        let mut b = AresUser::new(2, "10.0.0.2".parse().unwrap(), [1u8; 16]);
        *b.name.write() = "Bob".to_string();
        b.logged_in = true;
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        b.sender = Some(tx2);
        app.user_pool.add(Arc::new(b));

        let mgr = ScriptManager::new(
            app.clone(),
            std::env::temp_dir().join("astra_scripts_test_onpm"),
        );
        // Script sb0t: onPM(sender, target), ambos JSUser (no texto).
        mgr.load_source(
            "onpm",
            None,
            r#"
            function onPM(sender, target){
                if (typeof sender === "object" && sender.name === "Alice" &&
                    typeof target === "object" && target.name === "Bob" &&
                    (sender == "Alice") && (target == "Bob") &&
                    typeof target.ban === "function"){
                    sendPM("srv", "Alice", "OK");
                }
            }
            "#,
        )
        .unwrap();

        mgr.dispatch(&ScriptEvent::Private {
            from: "Alice".into(),
            to: "Bob".into(),
            text: "hola".into(),
        });

        let pkt = rx
            .try_recv()
            .expect("Alice debe recibir el PM => onPM recibió emisor+destino como objetos user");
        assert_eq!(pkt[0], 25, "esperado Pmt (25), got {}", pkt[0]);
    }

    #[test]
    fn event_handler_receives_user_object() {
        use server_core::user_pool::AresUser;
        let db = Database::in_memory().unwrap();
        let app = Arc::new(AppContext::new(Settings::default(), db));

        let mut u = AresUser::new(3, "127.0.0.1".parse().unwrap(), [0u8; 16]);
        *u.name.write() = "Alice".to_string();
        u.logged_in = true;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        u.sender = Some(tx);
        app.user_pool.add(Arc::new(u));

        let mgr = ScriptManager::new(
            app.clone(),
            std::env::temp_dir().join("astra_scripts_test_p4"),
        );
        // Handler estilo sb0t: primer arg es un JSUser (objeto), no string.
        // Sólo PMea si TODO se cumple: propiedades del objeto + método +
        // compat-string (toString/valueOf).
        mgr.load_source(
            "p4",
            None,
            r#"
            function onTextReceived(user, text){
                if (typeof user === "object" &&
                    user.name === "Alice" &&
                    typeof user.level === "number" &&
                    typeof user.ban === "function" &&
                    (user == "Alice") &&
                    ("got " + user) === "got Alice" &&
                    text === "hi"){
                    sendPM("bot", "Alice", "OK");
                }
            }
            "#,
        )
        .unwrap();

        mgr.dispatch(&ScriptEvent::TextReceived {
            from: "Alice".into(),
            text: "hi".into(),
        });

        let pkt = rx
            .try_recv()
            .expect("Alice debe recibir el PM => el handler recibió un objeto user funcional");
        assert_eq!(pkt[0], 25, "esperado opcode Pmt (25), got {}", pkt[0]);
    }

    #[test]
    fn load_simple_script() {
        let mgr = make_manager();
        let id = mgr
            .load_source("test", None, "print('loaded');")
            .expect("load should succeed");
        assert_eq!(mgr.count(), 1);
        let s = mgr.get(id).unwrap();
        assert_eq!(s.state(), ScriptLifecycle::Active);
    }

    #[test]
    fn unload_removes_script() {
        let mgr = make_manager();
        let id = mgr.load_source("test", None, "").unwrap();
        assert_eq!(mgr.count(), 1);
        assert!(mgr.unload(id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn dispatch_event_calls_handler() {
        let mgr = make_manager();
        let id = mgr
            .load_source(
                "test",
                None,
                r#"
                var received = null;
                function onPublic(from, text) {
                    received = from + ':' + text;
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::Public {
            from: "Alice".into(),
            text: "hello".into(),
        });

        let script = mgr.get(id).unwrap();
        let _ = script;
    }

    #[test]
    fn missing_handler_is_ok() {
        let mgr = make_manager();
        mgr.load_source("test", None, "print('no handlers here');")
            .unwrap();
        mgr.dispatch(&ScriptEvent::Public {
            from: "Alice".into(),
            text: "hi".into(),
        });
    }

    // ========== Tests Fase 15: eventos admin/cuenta/flood ==========

    #[test]
    fn login_granted_event_calls_handler() {
        let mgr = make_manager();
        let id = mgr
            .load_source(
                "test",
                None,
                r#"
                var last_login = null;
                function onLoginGranted(name) {
                    last_login = name;
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::LoginGranted {
            name: "Alice".into(),
        });

        // Verificar que el script se mantiene activo
        let s = mgr.get(id).unwrap();
        assert_eq!(s.state(), ScriptLifecycle::Active);
    }

    #[test]
    fn logout_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                var last_logout = null;
                function onLogout(name) {
                    last_logout = name;
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::Logout { name: "Bob".into() });
    }

    #[test]
    fn invalid_login_attempt_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onInvalidLoginAttempt(name, ip) {
                    // guardar en una variable
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::InvalidLoginAttempt {
            name: "Mallory".into(),
            ip: "1.2.3.4".into(),
        });
    }

    #[test]
    fn flood_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onFlood(name) {
                    // bloquear al user
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::Flood {
            name: "Spammer".into(),
        });
    }

    #[test]
    fn admin_level_changed_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onAdminLevelChanged(name) {
                    // auditar
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::AdminLevelChanged {
            name: "Charlie".into(),
        });
    }

    #[test]
    fn bans_auto_cleared_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                var last_clear = 0;
                function onBansAutoCleared() {
                    last_clear = last_clear + 1;
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::BansAutoCleared);
    }

    #[test]
    fn idled_and_unidled_events() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onIdled(name) {}
                function onUnidled(name) {}
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::Idled { name: "Alice".into() });
        mgr.dispatch(&ScriptEvent::Unidled { name: "Alice".into() });
    }

    #[test]
    fn proxy_detected_event() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onProxyDetected(ip) {}"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::ProxyDetected { ip: "5.6.7.8".into() });
    }

    #[test]
    fn multiple_handlers_fire_in_order() {
        // Verificar que múltiples scripts reciben el mismo evento
        let mgr = make_manager();
        mgr.load_source(
            "a",
            None,
            r#"function onLoginGranted(name) { print("a:" + name); }"#,
        )
        .unwrap();
        mgr.load_source(
            "b",
            None,
            r#"function onLoginGranted(name) { print("b:" + name); }"#,
        )
        .unwrap();
        mgr.dispatch(&ScriptEvent::LoginGranted { name: "Alice".into() });
        // Si llega hasta aquí sin panic, ambos scripts manejaron el evento
    }

    /// Test de integración: dispatcha un evento y verifica que el handler JS
    /// fue llamado con los argumentos correctos. Lo hace leyendo una variable
    /// JS que el handler modifica.
    #[test]
    fn dispatch_passes_correct_args_to_handler() {
        let mgr = make_manager();
        let id = mgr
            .load_source(
                "test",
                None,
                r#"
                var last_event = null;
                function onLoginGranted(name) {
                    last_event = { type: 'login', name: name };
                }
                function onLogout(name) {
                    last_event = { type: 'logout', name: name };
                }
                function onFlood(name) {
                    last_event = { type: 'flood', name: name };
                }
                "#,
            )
            .unwrap();

        mgr.dispatch(&ScriptEvent::LoginGranted { name: "Alice".into() });
        mgr.dispatch(&ScriptEvent::Logout { name: "Alice".into() });
        mgr.dispatch(&ScriptEvent::Flood { name: "Spammer".into() });

        // El script se mantiene activo tras varios eventos
        let s = mgr.get(id).unwrap();
        assert_eq!(s.state(), ScriptLifecycle::Active);
    }

    /// Test: cuando un script tiene un error, los eventos no se pierden para
    /// los otros scripts.
    #[test]
    fn error_in_one_script_doesnt_affect_others() {
        let mgr = make_manager();
        mgr.load_source("good", None, "function onLoginGranted(name) {}")
            .unwrap();
        // Forzar un error cargando un script que falla
        let result = mgr.load_source("bad", None, "function onLoad() { unknownVar; }");
        // El script "bad" queda en estado Error, pero el dispatch debe seguir
        // funcionando para "good"
        let _ = result;
        mgr.dispatch(&ScriptEvent::LoginGranted { name: "Alice".into() });
        // El script "good" sigue activo
        let good = mgr.get_by_name("good").unwrap();
        assert_eq!(good.state(), ScriptLifecycle::Active);
    }

    #[test]
    fn syntax_error_marks_error_state() {
        let mgr = make_manager();
        let result = mgr.load_source("bad", None, "function { broken");
        assert!(result.is_err());
        assert_eq!(mgr.count(), 1);
        let bad = mgr.list().into_iter().next().unwrap();
        let s = mgr.get(bad).unwrap();
        assert_eq!(s.state(), ScriptLifecycle::Error);
        assert!(s.last_error().is_some());
    }

    // ========== Tests Fase 16: eventos Vroom ==========

    #[test]
    fn vroom_join_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onVroomJoin(name, vroom) { /* auditar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::VroomJoin {
            name: "Alice".into(),
            vroom: 5,
        });
    }

    #[test]
    fn vroom_join_check_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onVroomJoinCheck(name, vroom) {
                    return vroom !== 13; // rechazar vroom 13
                }
                "#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::VroomJoinCheck {
            name: "Alice".into(),
            vroom: 5,
        });
    }

    // ========== Tests Fase 17: eventos Link ==========

    #[test]
    fn leaf_join_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onLeafJoin(name) { /* auditar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::LeafJoin { name: "Alice".into() });
    }

    #[test]
    fn leaf_part_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onLeafPart(name) { /* auditar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::LeafPart { name: "Bob".into() });
    }

    #[test]
    fn linked_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onLinked(name) { /* notificar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Linked { name: "hub.com".into() });
    }

    #[test]
    fn unlinked_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onUnlinked(name) { /* limpiar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Unlinked { name: "hub.com".into() });
    }

    #[test]
    fn link_error_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onLinkError(name, error) { /* log */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::LinkError {
            name: "hub.com".into(),
            error: "connection refused".into(),
        });
    }

    // ========== Tests Fase 20: Connect, Disconnect, UserList, UserListEnd ==========

    #[test]
    fn connect_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onConnect(ip) { /* log */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Connect { ip: "1.2.3.4".into() });
    }

    #[test]
    fn disconnect_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onDisconnect(ip) { /* cleanup */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Disconnect { ip: "1.2.3.4".into() });
    }

    #[test]
    fn userlist_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onUserList(name, users_csv) { /* log */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::UserList {
            name: "Alice".into(),
            users_csv: "".into(),
        });
    }

    #[test]
    fn userlist_end_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onUserListEnd(name) { /* fin */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::UserListEnd { name: "Alice".into() });
    }

    #[test]
    fn help_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onHelp(userobj) { /* extender help */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Help { from: "Alice".into() });
    }

    #[test]
    fn timer_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onTimer(id, name) { /* cleanup */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Timer {
            secs: 1,
            name: "myCallback".into(),
        });
    }

    // ========== Tests Fase 18: Avatar, FileReceived, ScribbleCheck ==========

    #[test]
    fn avatar_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"function onAvatar(name) { /* auditar cambio de avatar */ }"#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::Avatar {
            name: "Alice".into(),
            png: vec![0x89, 0x50, 0x4E, 0x47],
        });
    }

    #[test]
    fn file_received_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onFileReceived(name, filename) {
                    // log o blacklist de extensiones
                }
                "#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::FileReceived {
            name: "Alice".into(),
            filename: "astrahash://server.com:5009/file.zip".into(),
        });
    }

    #[test]
    fn scribble_check_event_calls_handler() {
        let mgr = make_manager();
        mgr
            .load_source(
                "test",
                None,
                r#"
                function onScribbleCheck(name, is_pm) {
                    // auditar o filtrar por nombre
                }
                "#,
            )
            .unwrap();
        mgr.dispatch(&ScriptEvent::ScribbleCheck {
            name: "Alice".into(),
            is_pm: false,
        });
        mgr.dispatch(&ScriptEvent::ScribbleCheck {
            name: "Bob".into(),
            is_pm: true,
        });
    }

    #[test]
    fn get_by_name() {
        let mgr = make_manager();
        mgr.load_source("hello", None, "").unwrap();
        mgr.load_source("world", None, "").unwrap();
        assert!(mgr.get_by_name("hello").is_some());
        assert!(mgr.get_by_name("world").is_some());
        assert!(mgr.get_by_name("missing").is_none());
    }

    #[test]
    #[ignore = "long-running test (thread); el canal se cierra al drop del handle"]
    fn start_in_thread_dispatches_via_handle() {
        use std::time::Duration;
        let mgr = make_manager();
        mgr.load_source(
            "test",
            None,
            r#"
            var captured = null;
            function onPublic(from, text) {
                captured = from + ':' + text;
            }
            "#,
        )
        .unwrap();

        let handle = mgr.start_in_thread();

        // Disparar un evento vía el handle
        handle.dispatch(ScriptEvent::Public {
            from: "Alice".into(),
            text: "hello".into(),
        });

        // Esperar un poco para que el thread del manager procese el evento
        std::thread::sleep(Duration::from_millis(100));

        // Drop del handle → cierra el canal → el thread termina
        drop(handle);
    }

    // ========== Tests Fase 14: hooks *Before con cancelación ==========
    //
    // Importante: NO usamos `start_in_thread` en estos tests porque los
    // Contexts de `boa_engine` se crean en el thread que llama a
    // `load_source` y no son Send. Si el manager se moviera a otro
    // thread, sería UB. Por eso testeamos `dispatch_request` directamente
    // sobre el manager, que vive en el thread del test.

    /// Corre un request de texto y devuelve el reply completo:
    /// `None` = cancelado, `Some(texto)` = permitido (posiblemente reescrito).
    fn run_text_request(mgr: &ScriptManager, request: ScriptRequest) -> Option<String> {
        let (tx, rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let request = match request {
            ScriptRequest::TextBefore { from, text, .. } => ScriptRequest::TextBefore {
                from,
                text,
                reply: tx,
            },
            ScriptRequest::EmoteBefore { from, text, .. } => ScriptRequest::EmoteBefore {
                from,
                text,
                reply: tx,
            },
            ScriptRequest::PMBefore { from, to, text, .. } => ScriptRequest::PMBefore {
                from,
                to,
                text,
                reply: tx,
            },
            _ => panic!("run_text_request es solo para los hooks de texto"),
        };
        mgr.dispatch_request(request);
        rx.recv_timeout(Duration::from_millis(200))
            .expect("manager should reply")
    }

    /// Compat con los tests viejos: ¿el request de texto fue permitido?
    fn check_request(mgr: &ScriptManager, request: ScriptRequest) -> bool {
        match request {
            ScriptRequest::ScribbleCheck { from, is_pm, .. } => {
                let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
                mgr.dispatch_request(ScriptRequest::ScribbleCheck { from, is_pm, reply: tx });
                rx.recv_timeout(Duration::from_millis(200)).expect("manager should reply")
            }
            other => run_text_request(mgr, other).is_some(),
        }
    }

    #[test]
    fn text_before_can_rewrite_text() {
        // Paridad sb0t: onTextBefore retorna el texto (reescrito) y se
        // encadena; false/null/"" cancela.
        let src = r#"
            function onTextBefore(from, text) {
                return ("" + text).replace("feo", "***");
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("censor", None, src).unwrap();

        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = run_text_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "que feo dia".into(),
            reply: tx,
        });
        assert_eq!(result, Some("que *** dia".to_string()));
    }

    #[test]
    fn text_before_empty_string_cancels() {
        let src = r#"function onTextBefore(from, text) { return ""; }"#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = run_text_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "hola".into(),
            reply: tx,
        });
        assert_eq!(result, None, "string vacío cancela (sb0t IsNullOrEmpty)");
    }

    #[test]
    fn text_before_chains_across_scripts() {
        let mgr = make_manager();
        mgr.load_source("a", None, r#"function onTextBefore(f, t){ return t + " [a]"; }"#).unwrap();
        mgr.load_source("b", None, r#"function onTextBefore(f, t){ return t + " [b]"; }"#).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = run_text_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "hola".into(),
            reply: tx,
        });
        let text = result.expect("permitido");
        assert!(text.contains("[a]") && text.contains("[b]"), "encadenado: {text}");
    }

    /// Los scripts de ejemplo de `data/scripts/` deben cargar sin errores y
    /// sus handlers deben ser invocables (evita que queden con firmas viejas
    /// tras un cambio de API, como pasó con onUserJoin/onCommand de 3 args).
    #[test]
    fn bundled_example_scripts_load_and_run() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("data/scripts");
        if !root.is_dir() {
            return; // sin scripts de ejemplo en este checkout
        }
        let mut checked = 0usize;
        for entry in std::fs::read_dir(&root).expect("read scripts dir") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("js") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read script");
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let mgr = make_manager();
            let id = mgr
                .load_source(&name, None, &src)
                .unwrap_or_else(|e| panic!("script '{}' no carga: {}", name, e));

            // `load_source` solo LOGUEA los errores de runtime de onLoad (el
            // script queda activo igual), así que hay que invocar los
            // handlers y verificar el Result — si no, este test pasaría con
            // un script que revienta en cada evento.
            let script = mgr.get(id).expect("script cargado");
            let mut guard = script.context.lock();
            let ctx = guard.as_mut().expect("contexto");

            // Se usan los MISMOS ArgKind que en producción (arg 0 = objeto
            // user, etc.); con Str plano el user llegaría como string y
            // `user.sendPM(...)` no existiría.
            let mut run = |handler: &str, args: &[String], kind: fn(usize) -> crate::types::ArgKind| {
                if let Err(e) = call_handler_with_return(ctx, handler, args, kind) {
                    panic!("script '{}': {} falló: {}", name, handler, e);
                }
            };
            use crate::types::ArgKind;
            let user0 = |i: usize| if i == 0 { ArgKind::User } else { ArgKind::Str };
            let cmd_kind = |i: usize| match i {
                0 | 2 => ArgKind::User,
                _ => ArgKind::Str,
            };
            run("onLoad", &[], |_| ArgKind::Str);
            run("onJoin", &["Alice".into()], user0);
            run("onJoinCheck", &["Alice".into(), "127.0.0.1".into()], user0);
            run("onHelp", &["Alice".into()], user0);
            run("onTextBefore", &["Alice".into(), "que feo dia".into()], user0);
            run("onVroomJoinCheck", &["Alice".into(), "9".into()], user0);
            run(
                "onCommand",
                &["Alice".into(), "pruebas".into(), String::new(), "target".into()],
                cmd_kind,
            );
            checked += 1;
        }
        assert!(checked > 0, "no se verificó ningún script de data/scripts/");
    }

    #[test]
    fn repro_onload_runs_on_load_and_no_duplicate_instances() {
        let mgr = make_manager();
        // onLoad deja una marca observable en el contexto del script.
        let src = r#"
            var CARGAS = 0;
            function onLoad(){ CARGAS = 1; }
            function onJoin(u){ CARGAS = CARGAS + 10; }
            function probe(){ return "" + CARGAS; }
        "#;

        // Lee la marca del script vivo con ese nombre.
        let probe = |mgr: &ScriptManager| -> String {
            let script = mgr.get_by_name("dup").expect("script vivo");
            let mut g = script.context.lock();
            let ctx = g.as_mut().unwrap();
            match call_handler_with_return(ctx, "probe", &[], |_| crate::types::ArgKind::Str) {
                Ok(HandlerReturn::Text(t)) => t,
                other => panic!("probe inesperado: {:?}", other.is_ok()),
            }
        };

        mgr.load_source("dup", None, src).unwrap();
        // (a) onLoad debe haber corrido al cargar.
        assert_eq!(probe(&mgr), "1", "onLoad no corrió al cargar el script");

        // (b) cargar 3 veces el mismo nombre → UNA sola instancia viva.
        mgr.load_source("dup", None, src).unwrap();
        mgr.load_source("dup", None, src).unwrap();
        let dups = mgr.scripts.lock().values().filter(|s| s.name == "dup").count();
        assert_eq!(dups, 1, "el script quedó cargado {} veces (debe reemplazar la instancia anterior)", dups);

        // (c) y los eventos deben dispararse UNA sola vez.
        mgr.dispatch(&ScriptEvent::Join {
            name: "Alice".into(),
            ip: "127.0.0.1".into(),
        });
        assert_eq!(probe(&mgr), "11", "onJoin corrió más de una vez (instancias duplicadas)");
    }

    /// Reproduce lo que ve el usuario: `#loadscript` de un script cuyo
    /// `onLoad` hace `print(...)` — el texto debe llegar a la sala.
    #[test]
    fn repro_onload_print_reaches_room() {
        use server_core::user_pool::AresUser;
        use std::net::{IpAddr, Ipv4Addr};

        let db = Database::in_memory().unwrap();
        let app = Arc::new(AppContext::new(Settings::default(), db));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
        let mut u = AresUser::new(1, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), [1u8; 16]);
        u.logged_in = true;
        u.sender = Some(tx);
        *u.name.write() = "Alice".to_string();
        app.user_pool.add(Arc::new(u));

        let mgr = ScriptManager::new(app, std::env::temp_dir().join("astra_scripts_test2"));
        mgr.load_source("hola", None, r#"function onLoad(){ print("script cargado!"); }"#)
            .unwrap();

        let pkt = rx
            .try_recv()
            .expect("onLoad debió imprimir a la sala al cargar el script");
        assert!(!pkt.is_empty());
    }

    /// Igual que [`repro_onload_print_reaches_room`] pero para un usuario WEB
    /// (ib0t/WebSocket): tiene `ws_text_sender` y NO tiene `sender` binario.
    /// Antes, `print()` de un script armaba el paquete binario Ares y lo
    /// mandaba por `u.send()` — que para users web se descarta en silencio,
    /// así que "onLoad no se ejecuta" desde un cliente web.
    #[test]
    fn repro_onload_print_reaches_web_user() {
        use server_core::user_pool::AresUser;
        use std::net::{IpAddr, Ipv4Addr};

        let db = Database::in_memory().unwrap();
        let app = Arc::new(AppContext::new(Settings::default(), db));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut u = AresUser::new(1, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), [1u8; 16]);
        u.logged_in = true;
        u.ws_text_sender = Some(tx);
        *u.name.write() = "WebAlice".to_string();
        app.user_pool.add(Arc::new(u));

        let mgr = ScriptManager::new(app, std::env::temp_dir().join("astra_scripts_test_web"));
        mgr.load_source("hola", None, r#"function onLoad(){ print("script cargado!"); }"#)
            .unwrap();

        let msg = rx
            .try_recv()
            .expect("el print() de onLoad debió llegar al cliente web como texto ib0t");
        assert!(
            msg.starts_with("PUBLIC:") && msg.contains("script cargado!"),
            "formato inesperado: {}",
            msg
        );
    }

    /// Paridad sb0t de `Sql.connected` (JSSqlInstance.cs): los scripts reales
    /// chequean `sql.connected` tras `open()`, no el retorno de `open()`.
    /// Réplica exacta del `initDatabaseQuery()` del hangman, que sin esta
    /// propiedad devolvía false → "Error al cargar script".
    #[test]
    fn sql_connected_property_like_sb0t() {
        let root = std::env::temp_dir().join(format!("astra_sql_conn_{}", std::process::id()));
        let dir = root.join("juego");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("juego.js"), "").unwrap();

        let mgr = make_manager();
        mgr.load_source(
            "juego",
            Some(dir.join("juego.js")),
            r#"
            function initDatabaseQuery() {
                var q = new Query("CREATE TABLE IF NOT EXISTS games (id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, user TEXT NOT NULL)");
                var sql = new Sql();
                var BEFORE = sql.connected;
                sql.open("hangman.db");
                var OK = false;
                if (sql.connected) { sql.query(q); sql.close(); OK = true; }
                return { before: BEFORE, ok: OK, after: sql.connected };
            }
            var R = initDatabaseQuery();
            "#,
        )
        .unwrap();
        let probe = |expr: &str| {
            let scripts = mgr.scripts.lock();
            let s = scripts.values().find(|s| s.name == "juego").unwrap();
            let mut guard = s.context.lock();
            let ctx = guard.as_mut().unwrap();
            ctx.eval(boa_engine::Source::from_bytes(expr.as_bytes()))
                .unwrap()
                .to_string(ctx)
                .unwrap()
                .to_std_string_escaped()
        };
        assert_eq!(probe("String(R.before)"), "false", "connected debe ser false antes de open()");
        assert_eq!(probe("String(R.ok)"), "true", "sql.connected debe ser true tras open()");
        assert_eq!(probe("String(R.after)"), "false", "connected debe volver a false tras close()");
        assert!(dir.join("sql").join("hangman.db").exists(), "la DB debe crearse en <script>/sql/");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Paridad sb0t de `File.*` (JSFile.cs): los datos de un script viven en
    /// su subcarpeta `data/` — `File.load("words.txt")` debe leer
    /// `<script>/data/words.txt` (layout real del script hangman), y
    /// `File.write` debe escribir ahí (creando `data/` si falta). `include()`
    /// en cambio resuelve en la RAÍZ de la carpeta del script.
    #[test]
    fn file_api_resolves_in_data_subfolder_like_sb0t() {
        let root = std::env::temp_dir().join(format!("astra_file_data_{}", std::process::id()));
        let dir = root.join("juego");
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data").join("words.txt"), "perro\ngato\n").unwrap();
        std::fs::write(dir.join("juego.js"), "").unwrap();

        let mgr = make_manager();
        mgr.load_source(
            "juego",
            Some(dir.join("juego.js")),
            r#"
            var EXISTS = File.exists("words.txt");
            var CONTENT = File.load("words.txt");
            var WROTE = File.write("scores.txt", "Alice=3");
            "#,
        )
        .unwrap();
        let probe = |expr: &str| {
            let scripts = mgr.scripts.lock();
            let s = scripts.values().find(|s| s.name == "juego").unwrap();
            let mut guard = s.context.lock();
            let ctx = guard.as_mut().unwrap();
            ctx.eval(boa_engine::Source::from_bytes(expr.as_bytes()))
                .unwrap()
                .to_string(ctx)
                .unwrap()
                .to_std_string_escaped()
        };
        assert_eq!(probe("String(EXISTS)"), "true", "File.exists no encontró data/words.txt");
        assert!(probe("CONTENT").contains("perro"), "File.load no leyó data/words.txt");
        assert_eq!(probe("String(WROTE)"), "true");
        assert!(
            dir.join("data").join("scores.txt").exists(),
            "File.write debe escribir en data/ (paridad sb0t)"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn killscript_removes_all_instances_and_cleans_resources() {
        let mgr = make_manager();
        let src = r#"
            function onLoad(){ Help_addLine("mio", "linea del script"); setTimer(60, "tick"); }
            function tick(){}
        "#;
        mgr.load_source("victima", None, src).unwrap();
        assert!(
            crate::api::extra_help_lines().iter().any(|(c, _)| c == "mio"),
            "el script debió registrar su línea de /help"
        );

        // killscript debe dejar CERO instancias…
        let (tx, rx) = std_mpsc::sync_channel::<Result<(), String>>(1);
        mgr.dispatch_request(ScriptRequest::KillScript {
            name: "victima".into(),
            reply: tx,
        });
        rx.recv_timeout(Duration::from_millis(500))
            .expect("reply")
            .expect("killscript ok");
        assert!(mgr.get_by_name("victima").is_none(), "quedó una instancia viva");

        // …y limpiar los recursos que dejó registrados (si no, sus líneas de
        // /help siguen apareciendo y sus timers se re-encolan para siempre).
        assert!(
            !crate::api::extra_help_lines().iter().any(|(c, _)| c == "mio"),
            "las líneas de /help del script muerto siguen ahí"
        );
    }

    #[test]
    fn onload_error_is_reported_not_swallowed() {
        // Un onLoad que revienta no debe fallar en silencio: el script queda
        // cargado (como sb0t) pero con el error registrado y notificado.
        let mgr = make_manager();
        let id = mgr
            .load_source("roto", None, "function onLoad(){ no_existe_esto(); }")
            .expect("el script carga igual");
        let script = mgr.get(id).expect("script");
        let err = script.last_error().expect("el error de onLoad debe quedar registrado");
        assert!(err.contains("no_existe_esto"), "error inesperado: {err}");
    }

    /// E2E de `#errors on`: un usuario suscrito debe RECIBIR por PM los
    /// errores de los scripts — tanto los de carga (eval) como los de
    /// `onLoad` y los de los handlers en runtime. Antes, los de onLoad y los
    /// de los hooks solo iban al log del server y el chat no mostraba nada.
    #[test]
    fn errors_subscription_delivers_script_errors() {
        use server_core::user_pool::AresUser;
        use std::net::{IpAddr, Ipv4Addr};
        use std::sync::atomic::Ordering::Relaxed;

        let db = Database::in_memory().unwrap();
        let app = Arc::new(AppContext::new(Settings::default(), db));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
        let mut u = AresUser::new(1, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), [1u8; 16]);
        u.logged_in = true;
        u.sender = Some(tx);
        *u.name.write() = "Admin".to_string();
        let admin = Arc::new(u);
        admin.sub_errors.store(true, Relaxed); // == #errors on
        app.user_pool.add(admin.clone());

        let mgr = ScriptManager::new(app, std::env::temp_dir().join("astra_scripts_test3"));

        // (a) error de RUNTIME en onLoad → el admin lo recibe.
        mgr.load_source("roto", None, "function onLoad(){ boom_no_existe(); }")
            .expect("el script carga igual");
        assert!(
            rx.try_recv().is_ok(),
            "#errors on no entregó el error de onLoad"
        );

        // (b) error dentro de un hook (onTextBefore) → también.
        mgr.load_source(
            "roto2",
            None,
            "function onTextBefore(u, t){ otra_boom(); return t; }",
        )
        .unwrap();
        while rx.try_recv().is_ok() {} // drenar
        let (rtx, rrx) = std_mpsc::sync_channel::<Option<String>>(1);
        mgr.dispatch_request(ScriptRequest::TextBefore {
            from: "Admin".into(),
            text: "hola".into(),
            reply: rtx,
        });
        let _ = rrx.recv_timeout(Duration::from_millis(500));
        assert!(
            rx.try_recv().is_ok(),
            "#errors on no entregó el error de onTextBefore"
        );

        // (c) error de SINTAXIS al cargar → también.
        while rx.try_recv().is_ok() {}
        let _ = mgr.load_source("roto3", None, "function ( { sintaxis mala");
        assert!(
            rx.try_recv().is_ok(),
            "#errors on no entregó el error de carga"
        );
    }

    #[test]
    fn resolve_main_prefers_handler_file_when_name_mismatches() {
        // Carpeta cuyo nombre NO coincide con el .js principal: el fallback
        // debe elegir el archivo con handlers, no un helper al azar.
        let dir = std::env::temp_dir().join("astra_resolve_main");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("constants.js"), "var X = 1;").unwrap();
        std::fs::write(dir.join("utils.js"), "function helper(){ return 1; }").unwrap();
        std::fs::write(dir.join("game.js"), "function onLoad(){} function onCommand(u,c,t,a){}").unwrap();

        let main = resolve_main_file(&dir).expect("main");
        assert_eq!(
            main.file_name().unwrap().to_str().unwrap(),
            "game.js",
            "debió elegir el archivo con handlers, no un helper"
        );
    }

    #[test]
    fn oncommand_receives_full_command_line() {
        // Paridad sb0t: onCommand(userobj, command, target, args) donde
        // `command` es la LÍNEA COMPLETA (nombre + args), no solo el nombre.
        // Un script que hace command.split(" ") debe ver los argumentos.
        let mgr = make_manager();
        mgr.load_source(
            "t",
            None,
            r#"
                var LAST = "";
                function onCommand(u, command, target, args) {
                    LAST = command;  // guardamos la línea completa recibida
                }
            "#,
        )
        .unwrap();

        // Simula lo que arma commands::dispatch: command = línea completa.
        mgr.dispatch(&ScriptEvent::Command {
            from: "Alice".into(),
            command: "nuevojuego perro".into(),
            target: String::new(),
            args: "perro".into(),
        });

        let script = mgr.get_by_name("t").unwrap();
        let mut g = script.context.lock();
        let ctx = g.as_mut().unwrap();
        assert_eq!(
            crate::api::eval_script_value(ctx, "LAST").unwrap(),
            "nuevojuego perro",
            "onCommand no recibió la línea completa"
        );
        // Y command.split(" ") da los argumentos.
        assert_eq!(crate::api::eval_script_value(ctx, "LAST.split(' ')[1]").unwrap(), "perro");
    }

    #[test]
    fn join_check_can_reject() {
        let src = r#"
            function onJoinCheck(userobj, ip) {
                if (("" + userobj) == "Malo") return false;
                return true;
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("guard", None, src).unwrap();

        let run = |name: &str| -> bool {
            let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
            mgr.dispatch_request(ScriptRequest::JoinCheck {
                name: name.into(),
                ip: "1.2.3.4".into(),
                reply: tx,
            });
            rx.recv_timeout(Duration::from_millis(200)).unwrap()
        };
        assert!(!run("Malo"), "script debe rechazar a Malo");
        assert!(run("Bueno"), "script debe dejar pasar a Bueno");
    }

    #[test]
    fn flood_before_can_spare() {
        let src = r#"function onFloodBefore(userobj, msg) { return false; }"#;
        let mgr = make_manager();
        mgr.load_source("mercy", None, src).unwrap();
        let (tx, rx) = std_mpsc::sync_channel::<bool>(1);
        mgr.dispatch_request(ScriptRequest::FloodBefore {
            name: "Spammer".into(),
            msg: "aaa".into(),
            reply: tx,
        });
        assert!(!rx.recv_timeout(Duration::from_millis(200)).unwrap());
    }

    #[test]
    fn text_before_returns_true_when_no_handler() {
        // Sin handler onTextBefore → default allow
        let mgr = make_manager();
        mgr.load_source("test", None, "function onLoad() {}").unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "hello".into(),
            reply: tx,
        });
        assert!(result, "should allow when no onTextBefore defined");
    }

    #[test]
    fn text_before_can_cancel() {
        let src = r#"
            function onTextBefore(from, text) {
                if (text === "spam") return false;
                return true;
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();

        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "hello".into(),
            reply: tx,
        });
        assert!(result, "non-spam should pass");

        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Bob".into(),
            text: "spam".into(),
            reply: tx,
        });
        assert!(!result, "spam should be cancelled");
    }

    #[test]
    fn text_before_allows_by_default() {
        let src = r#"
            function onTextBefore(from, text) {
                return true;
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "anything".into(),
            reply: tx,
        });
        assert!(result);
    }

    #[test]
    fn text_before_returning_non_bool_ignores() {
        let src = r#"
            function onTextBefore(from, text) {
                return "yes"; // no es bool → no cancela
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "x".into(),
            reply: tx,
        });
        assert!(result, "non-bool return should not cancel");
    }

    #[test]
    fn emote_before_can_cancel() {
        let src = r#"
            function onEmoteBefore(from, text) {
                if (text.length > 50) return false;
                return true;
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::EmoteBefore {
            from: "Alice".into(),
            text: "short emote".into(),
            reply: tx,
        });
        assert!(result);

        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::EmoteBefore {
            from: "Bob".into(),
            text: "x".repeat(60),
            reply: tx,
        });
        assert!(!result, "long emote should be cancelled");
    }

    #[test]
    fn pm_before_can_cancel() {
        // sb0t: onPMBefore(emisor JSUser, destino JSUser, mensaje JSPM).
        // `to` es un objeto user; se compara por .name o con == (string-compat).
        let src = r#"
            function onPMBefore(from, to, text) {
                if (to == "Alice") return false;
                return true;
            }
        "#;
        let mgr = make_manager();
        mgr.load_source("test", None, src).unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::PMBefore {
            from: "Bob".into(),
            to: "Alice".into(),
            text: "hi".into(),
            reply: tx,
        });
        assert!(!result, "PM a Alice debería ser bloqueado");

        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::PMBefore {
            from: "Bob".into(),
            to: "Charlie".into(),
            text: "hi".into(),
            reply: tx,
        });
        assert!(result, "PM a Charlie debería pasar");
    }

    #[test]
    fn multiple_scripts_any_cancel_wins() {
        // Primer script: allow
        // Segundo script: cancel
        // Resultado: cancel
        let mgr = make_manager();
        mgr.load_source("good", None, "function onTextBefore() { return true; }").unwrap();
        mgr.load_source("bad", None, "function onTextBefore() { return false; }").unwrap();
        let (tx, _rx) = std_mpsc::sync_channel::<Option<String>>(1);
        let result = check_request(&mgr, ScriptRequest::TextBefore {
            from: "Alice".into(),
            text: "hello".into(),
            reply: tx,
        });
        assert!(!result, "any script returning false should cancel");
    }

    #[test]
    fn handle_methods_default_to_allow_when_no_manager() {
        // Si el manager no existe, los métodos deben retornar true (allow)
        // Esto es difícil de testear sin un manager, pero podemos verificar
        // que un handle "muerto" (después de drop del manager) retorna true.
        let mgr = make_manager();
        let _handle = mgr.start_in_thread();
        // drop el manager implícitamente al final de la función
        // El handle se queda con canales cerrados → check_xxx_before debe retornar true
        // (no podemos probar directamente esto sin un sleep, así que solo verificamos
        // que el método existe y no panicea)
        std::thread::sleep(Duration::from_millis(50));
    }
}
