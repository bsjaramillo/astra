//! ObjectPrototypes (constructores de tipos para los Objects).
//!
//! Son equivalentes a los Objects pero con un nombre de "Prototype"
//! para compatibilidad con el sb0t original.

use std::sync::Arc;
use boa_engine::{Context, JsValue};

use super::super::ScriptState;
use super::register_fn;

pub fn register(ctx: &mut Context, _state: Arc<ScriptState>) {
    register_fn(ctx, "AvatarImage_new", 2, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{name:\"{}\",hash:\"\"}}", name)))
    });
    register_fn(ctx, "BannedUser_new", 5, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("BannedUser:{{name:\"{}\"}}", name)))
    });
    register_fn(ctx, "ChannelCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "Channel_new", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("Channel:{{name:\"{}\"}}", name)))
    });
    register_fn(ctx, "CryptoResult_new", 2, |_this, args, _ctx| {
        let hash = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{hash:\"{}\"}}", hash)))
    });
    register_fn(ctx, "HashlinkResult_new", 6, |_this, args, _ctx| {
        let name = args.get(2).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("HashlinkResult:{{name:\"{}\"}}", name)))
    });
    register_fn(ctx, "HttpRequestResult_new", 4, |_this, args, _ctx| {
        let body = args.get(3).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{body:\"{}\"}}", body)))
    });
    register_fn(ctx, "IgnoreCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "Leaf_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from("{}"))
    });
    register_fn(ctx, "NodeAttributes_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from("{}"))
    });
    register_fn(ctx, "NodeCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "Node_new", 1, |_this, args, _ctx| {
        Ok(JsValue::from(format!("Node:{{}}", args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default())))
    });
    register_fn(ctx, "PM_new", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("PM:{{name:\"{}\"}}", name)))
    });
    register_fn(ctx, "ProxyCheckResult_new", 2, |_this, args, _ctx| {
        let level = args.get(1).and_then(|v| v.as_number()).map(|n| n as u8).unwrap_or(0);
        Ok(JsValue::from(format!("{{level:{}}}", level)))
    });
    register_fn(ctx, "Record_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from("{}"))
    });
    register_fn(ctx, "RegistryKeyCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "ScribbleImage_new", 3, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{name:\"{}\",hash:\"\",height:0}}", name)))
    });
    register_fn(ctx, "SpellingSuggestionCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "User_new", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("User:{{name:\"{}\"}}", name)))
    });
}
