//! Bindings JS para scripts — versión compatible con el sb0t original.
//!
//! Cada submódulo corresponde a una clase/objeto estático del API
//! del sb0t. Los scripts que usen estos nombres (ej. `Base64.encode(x)`)
//! deberían funcionar sin cambios.
//!
//! ## Estado
//!
//! - **Statics**: `Base64`, `Channels`, `Crypto`, `Entities`, `File`,
//!   `Hashlink`, `Link`, `Registry`, `Room`, `ScriptInclude`,
//!   `Spelling`, `Stats`, `Users`, `Zip` — stubs
//! - **Objects**: `Avatar`, `BannedUser`, `Channel`, `ChannelCollection`,
//!   `HashlinkResult`, `HttpRequestResult`, `IgnoreCollection`,
//!   `Leaf`, `Node`, `NodeAttributes`, `NodeCollection`, `PM`,
//!   `ProxyCheckResult`, `Record`, `RegistryKeyCollection`,
//!   `ScribbleImage`, `SpellingSuggestionCollection`, `User` — stubs
//! - **Instances**: constructores para los Objects
//!
//! Estos bindings son **stubs básicos** que devuelven datos de muestra.
//! La integración real con el servidor (leer userlist, crear avatares,
//! etc.) queda como TODO en cada módulo.

#![allow(unused_variables)]

use boa_engine::{js_string, Context, JsValue, NativeFunction};
use std::sync::Arc;

use super::super::ScriptState;

/// Registra todos los bindings JS (Statics + Properties + Object constructors).
pub fn register_all(ctx: &mut Context, state: Arc<ScriptState>) {
    statics::register(ctx, state.clone());
    properties::register(ctx, state.clone());
    objects::register(ctx, state);
    prototypes::register(ctx);
    instances::register(ctx);
}

mod statics;
mod properties;
mod objects;
mod prototypes;
mod instances;

// Helper para registrar una función estática global
fn register_fn(
    ctx: &mut Context,
    name: &str,
    argc: usize,
    func: fn(&JsValue, &[JsValue], &mut Context) -> Result<JsValue, boa_engine::JsError>,
) {
    let native = NativeFunction::from_fn_ptr(func);
    ctx.register_global_builtin_callable(js_string!(name), argc, native)
        .expect("failed to register global callable");
}
