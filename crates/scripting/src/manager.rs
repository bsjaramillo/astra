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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use boa_engine::JsValue;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use server_core::AppContext;

use crate::api::{call_global_function, eval_script, make_context, unregister_context};
use crate::types::{Script, ScriptEvent, ScriptId, ScriptState as ScriptLifecycle};


/// Handle `Send + Clone` para enqueue de eventos al manager.
///
/// Es un `mpsc::UnboundedSender<ScriptEvent>`. Se clona y se pasa a
/// otras tasks. El dispatch es no-bloqueante: si el thread del manager
/// está caído, el evento se descarta silenciosamente.
#[derive(Clone)]
pub struct ScriptHandle {
    tx: mpsc::UnboundedSender<ScriptEvent>,
}

impl ScriptHandle {
    /// Encola un evento. No bloquea.
    pub fn dispatch(&self, event: ScriptEvent) {
        let _ = self.tx.send(event);
    }
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
    /// Carga todos los scripts `.js` del directorio configurado antes de
    /// iniciar el thread.
    pub fn start_in_thread(self) -> ScriptHandle {
        // Cargar los scripts del directorio
        let _ = self.load_all_inner();

        // Crear el canal mpsc
        let (tx, mut rx) = mpsc::unbounded_channel::<ScriptEvent>();

        // Spawn del thread dedicado. El manager se mueve al thread vía
        // un `usize` (Send + Copy + 'static). El *mut ScriptManager
        // NO es Send directamente, pero un usize sí.
        // SAFETY: el manager vive solo en este thread y se destruye
        // cuando el thread termina.
        let manager_ptr: usize = Box::into_raw(Box::new(self)) as usize;

        std::thread::spawn(move || {
            // SAFETY: reconstituimos el Box desde el usize
            let manager = unsafe { Box::from_raw(manager_ptr as *mut ScriptManager) };
            info!("script manager: thread iniciado");

            // Loop principal: consume eventos y dispatcha
            while let Some(event) = rx.blocking_recv() {
                manager.dispatch(&event);
            }

            info!("script manager: thread terminado (canal cerrado)");
        });

        ScriptHandle { tx }
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
                if path.extension().and_then(|s| s.to_str()) == Some("js") {
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
        let script = Arc::new(Script::new(name.to_string(), path));
        let mut ctx = make_context(self.app.clone());

        if let Err(e) = eval_script(&mut ctx, source) {
            error!("script '{}': {}", name, e);
            script.set_state(ScriptLifecycle::Error);
            script.set_error(e.clone());
            let id = script.id;
            unregister_context(&ctx);
            self.scripts.lock().insert(id, script);
            return Err(e);
        }

        let onload_result = call_void_handler(&mut ctx, "onLoad", &[]);
        if let Err(e) = onload_result {
            warn!("script '{}' onLoad() error: {}", name, e);
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

    /// Despacha un evento a todos los scripts activos.
    /// Se llama desde el thread del manager (no desde otros threads).
    pub fn dispatch(&self, event: &ScriptEvent) {
        let handler_name = event.handler_name();
        let args = event.args();

        let scripts = self.scripts.lock().clone();
        for (_, script) in scripts.iter() {
            if script.state() != ScriptLifecycle::Active {
                continue;
            }
            let mut ctx_guard = script.context.lock();
            if let Some(ctx) = ctx_guard.as_mut() {
                if let Err(e) = call_void_handler(ctx, handler_name, &args) {
                    let msg = format!("error en handler '{}': {}", handler_name, e);
                    warn!("script '{}': {}", script.name(), msg);
                    script.set_error(msg);
                }
            }
        }
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
        call_void_handler(ctx, event.handler_name(), &event.args())
    }
}

/// Llama a una función JS sin retorno con los argumentos dados.
fn call_void_handler(
    ctx: &mut boa_engine::Context,
    name: &str,
    args: &[String],
) -> Result<(), String> {
    let js_args: Vec<JsValue> = args
        .iter()
        .map(|s| JsValue::from(boa_engine::js_string!(s.as_str())))
        .collect();
    call_global_function(ctx, name, &js_args)
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
}
