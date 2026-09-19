//! Admisión compartida entre los transports TCP nativo y WebSocket/ib0t.
//!
//! Históricamente el login TCP (`process_handshake` en `crates/astra`) y el
//! login web (`ws_handshake_login` en `crates/web`) evolucionaron por
//! separado, y el path web terminó **sin varios gates** que el nativo sí
//! aplica: validación del login (Capa 4), gate de proxy, range bans, join
//! filters, ASN bans, captcha y los límites de conexión de las Capas 2/5.
//!
//! Este crate centraliza **el orden y la semántica de los gates** una sola
//! vez. Cada transport solo traduce el [`Admission`] resultante a su formato
//! de error (`ServerError` binario vs `"ERROR:..."` de texto) y registra los
//! eventos de scripting/stats. Así, añadir un gate nuevo (p. ej. el filtro
//! anti-VPN) lo aplica a ambos por construcción, en vez de depender de que
//! alguien recuerde portarlo.
//!
//! El gate `onJoinCheck` posterior al alta del usuario también se expone aquí
//! ([`join_allowed`]) porque estaba duplicado en ambos transports.

use std::net::IpAddr;

use server_core::login::LoginData;
use server_core::security::RejectReason;
use server_core::types::ILevel;
use server_core::user_pool::AresUser;
use server_core::{AppContext, VpnAction};

/// Resultado de evaluar la admisión de un login, antes de crear el usuario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// El login es admisible. `hijacked` indica que se reemplazó una sesión
    /// previa con el mismo nick y la misma IP.
    Allow {
        /// Se reemplazó una sesión previa (mismo nick + misma IP).
        hijacked: bool,
    },
    /// Rechazar. `kind` es la causa (para stats/eventos) y `client_message`
    /// el texto a enviar al cliente.
    Reject {
        /// Razón estructurada del rechazo.
        kind: RejectReason,
        /// Mensaje user-friendly para el cliente.
        client_message: String,
    },
    /// Dejar entrar pero exigir captcha antes de poder hablar.
    Captcha {
        /// Prompt ya formateado para enviar por PM del bot.
        prompt: String,
        /// Se reemplazó una sesión previa (mismo nick + misma IP).
        hijacked: bool,
    },
}

/// Evalúa los gates de entrada de un login. **No crea el usuario**: eso lo
/// hace cada transport después de recibir [`Admission::Allow`].
///
/// Orden (idéntico para TCP y web):
/// 1. validación de campos (Capa 4) + gate de proxy (`onProxyDetected`);
/// 2. ban persistente;
/// 3. range ban;
/// 4. join filter;
/// 5. ASN ban;
/// 6. filtro anti-VPN/proxy;
/// 7. join-flood;
/// 8. nick en uso / hijack.
///
/// El "local host" (paridad sb0t) queda exento de los gates 2-6.
pub fn evaluate(
    ctx: &AppContext,
    login: &LoginData,
    ip: IpAddr,
    scripting: &astra_scripting::ScriptHandle,
) -> Admission {
    let is_local = ctx.is_local_host(ip);
    let name = &login.org_name;

    // ── 1. Validación de campos (Capa 4) + issue de proxy ──────────────
    let (validation, issues) = ctx.security.login_validator.validate(login);
    for issue in &issues {
        match issue {
            server_core::security::DetectedIssue::Proxy => {
                // sb0t parity: gate onProxyDetected. Si un script rechaza,
                // se corta el login aquí. El retorno default es `false`, así
                // que sin scripts que lo habiliten esto rechaza (igual que sb0t).
                let allowed = scripting.check_proxy_detected(name, &ip.to_string());
                scripting.dispatch(astra_scripting::ScriptEvent::ProxyDetected {
                    name: name.clone(),
                    ip: ip.to_string(),
                    reply: allowed,
                });
                if !allowed {
                    return Admission::Reject {
                        kind: RejectReason::SuspiciousProfile,
                        client_message: "Connection rejected.".to_string(),
                    };
                }
            }
        }
    }
    if let Err(reason) = validation {
        return Admission::Reject {
            kind: reason,
            client_message: reason.message().to_string(),
        };
    }

    // ── 2. Ban persistente ─────────────────────────────────────────────
    if !is_local && ctx.bans.is_banned(&login.guid, ip) {
        return reject_generic();
    }

    // ── 3. Range ban ───────────────────────────────────────────────────
    if !is_local && ctx.range_bans.is_banned(ip) {
        return reject_generic();
    }

    // ── 4. Join filter ─────────────────────────────────────────────────
    if ctx.join_filters.matches(name) {
        return Admission::Reject {
            kind: RejectReason::InvalidName,
            client_message: "Your nickname is not allowed here".to_string(),
        };
    }

    // ── 5. ASN ban ─────────────────────────────────────────────────────
    if !is_local && !ctx.asn_bans.is_empty() {
        if let Some(asn) = ctx.geoip.lookup_asn(ip) {
            if ctx.asn_bans.is_banned(asn) {
                return reject_generic();
            }
        }
    }

    // ── 6. Filtro anti-VPN/proxy ───────────────────────────────────────
    // `captcha`/`quarantine` NO cortan el flujo: aún deben pasar join-flood y
    // nick/hijack (si no, un VPN con nick duplicado se colaría). Solo se
    // recuerda la decisión para el resultado final.
    let mut vpn_captcha = false;
    if !is_local {
        if let Some(hit) = ctx.vpn_filter.classify(&ctx.geoip, ip) {
            let action = ctx.vpn_filter.action();
            tracing::warn!(
                "VPN/proxy detectado: ip={} nick='{}' regla={:?}:{} accion={:?}",
                ip,
                name,
                hit.kind,
                hit.rule,
                action
            );
            scripting.dispatch(astra_scripting::ScriptEvent::VpnDetected {
                name: name.clone(),
                ip: ip.to_string(),
                rule: hit.rule.clone(),
                action: action.as_str().to_string(),
            });
            match action {
                VpnAction::Report | VpnAction::Quarantine => {
                    // `quarantine` se aplica tras crear el usuario
                    // (`apply_vpn_quarantine`).
                }
                VpnAction::Reject => {
                    return Admission::Reject {
                        kind: RejectReason::VpnBlocked,
                        client_message: "Connection rejected.".to_string(),
                    };
                }
                VpnAction::Captcha => {
                    vpn_captcha = true;
                }
            }
        }
    }

    // ── 7. Join-flood ──────────────────────────────────────────────────
    let now_ms = server_core::time::unix_time();
    if ctx.user_history.is_join_flooding(ip, now_ms) {
        return Admission::Reject {
            kind: RejectReason::ConnectionFlood,
            client_message: "Joining too quickly. Please wait 15 seconds.".to_string(),
        };
    }

    // ── 8. Nick en uso / hijack ────────────────────────────────────────
    let hijacked = if let Some(existing) = ctx.user_pool.get_by_name(name) {
        if existing.external_ip == ip {
            // Misma IP: reconexión. La sesión vieja se retira como ghost
            // (sin PART) y el login sigue.
            ctx.ghost_part_user(&existing);
            true
        } else {
            return Admission::Reject {
                kind: RejectReason::InvalidName,
                client_message: "Nickname already in use".to_string(),
            };
        }
    } else {
        false
    };

    if vpn_captcha {
        return Admission::Captcha {
            prompt: captcha_prompt(ctx),
            hijacked,
        };
    }

    Admission::Allow { hijacked }
}

fn reject_generic() -> Admission {
    Admission::Reject {
        kind: RejectReason::ConnectionFloodBan,
        client_message: "You are banned from this room".to_string(),
    }
}

/// Marca la cuarentena del filtro anti-VPN si la acción configurada es
/// `quarantine` y la IP matchea. Se llama **después** de crear el usuario,
/// porque necesita el `AresUser`.
///
/// Retorna `true` si aplicó cuarentena.
pub fn apply_vpn_quarantine(ctx: &AppContext, user: &AresUser) -> bool {
    if !ctx.vpn_filter.is_enabled() || ctx.vpn_filter.action() != VpnAction::Quarantine {
        return false;
    }
    if ctx.is_local_host(user.external_ip) {
        return false;
    }
    if ctx.vpn_filter.is_blocked(&ctx.geoip, user.external_ip) {
        user.quarantined
            .store(true, std::sync::atomic::Ordering::Relaxed);
        return true;
    }
    false
}

/// Gate `onJoinCheck` (paridad sb0t `Joining`): un script puede rechazar el
/// join. El "local host" no puede ser rechazado, para que un script con un
/// bug no deje al dueño fuera de su sala.
pub fn join_allowed(
    ctx: &AppContext,
    user: &AresUser,
    scripting: &astra_scripting::ScriptHandle,
) -> bool {
    if ctx.is_local_host(user.external_ip) {
        return true;
    }
    scripting.check_join(&user.name.read().clone(), &user.external_ip.to_string())
}

// ============================================================================
// Captcha
// ============================================================================

/// Texto del prompt de captcha, con la palabra ofuscada. Comparte el mismo
/// formato en TCP y web.
pub fn captcha_prompt(ctx: &AppContext) -> String {
    format!(
        "Welcome! Please type this code to enter: {}  (PM it back to {})",
        "{{code}}", ctx.settings.bot_name
    )
}

/// Emite un captcha para el usuario: crea el challenge, marca los flags y
/// retorna el prompt listo para enviar por PM del bot.
pub fn issue_captcha(ctx: &AppContext, user: &AresUser) -> String {
    let user_id = user.id.to_string();
    let challenge = ctx.captcha.create(user_id);
    user.needs_captcha
        .store(true, std::sync::atomic::Ordering::Relaxed);
    user.quarantined
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let visual = obfuscate_captcha_word(&challenge.word);
    format!(
        "Welcome! Please type this code to enter: {}  (PM it back to {})",
        visual, ctx.settings.bot_name
    )
}

/// Resultado de procesar la respuesta de un captcha en un transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptchaOutcome {
    /// Resuelto: se limpiaron los flags.
    Solved,
    /// Incorrecto, quedan intentos. `remaining` intentos restantes.
    Wrong {
        /// Intentos restantes antes del kick.
        remaining: u32,
    },
    /// Expiró o se agotaron los intentos: el caller debe kickear.
    Failed,
    /// No había challenge pendiente (no hay nada que hacer).
    NoChallenge,
}

/// Verifica la respuesta de captcha de un usuario y actualiza sus flags.
///
/// En éxito limpia `needs_captcha` y `quarantined`. Devuelve el outcome para
/// que el transport decida si avisar intentos restantes o kickear.
pub fn verify_captcha(ctx: &AppContext, user: &AresUser, answer: &str) -> CaptchaOutcome {
    use server_core::captcha::VerifyResult;
    let user_id = user.id.to_string();
    match ctx.captcha.verify(&user_id, answer) {
        VerifyResult::Ok => {
            user.needs_captcha
                .store(false, std::sync::atomic::Ordering::Relaxed);
            user.quarantined
                .store(false, std::sync::atomic::Ordering::Relaxed);
            // La cuarentena por VPN no debe sobrevivir a un captcha resuelto.
            ctx.captcha.clear(&user_id);
            CaptchaOutcome::Solved
        }
        VerifyResult::Wrong { remaining } => CaptchaOutcome::Wrong { remaining },
        VerifyResult::Expired | VerifyResult::TooManyAttempts => {
            // Se agotó: limpiar consistencia y dejar que el transport kickee.
            user.needs_captcha
                .store(false, std::sync::atomic::Ordering::Relaxed);
            ctx.captcha.clear(&user_id);
            CaptchaOutcome::Failed
        }
        VerifyResult::AlreadyCompleted => {
            // Ya resuelto: no volver a marcar error.
            user.needs_captcha
                .store(false, std::sync::atomic::Ordering::Relaxed);
            user.quarantined
                .store(false, std::sync::atomic::Ordering::Relaxed);
            CaptchaOutcome::Solved
        }
        VerifyResult::NoChallenge => CaptchaOutcome::NoChallenge,
    }
}

/// Ofusca la palabra del captcha (paridad sb0t: intercala la palabra con
/// caracteres aleatorios para que un OCR simple no la lea del texto plano).
fn obfuscate_captcha_word(word: &str) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let noise = ['#', '*', '.', '-', '+', '~', '=', '^'];
    let mut out = String::with_capacity(word.len() * 3);
    for c in word.chars() {
        out.push(c);
        let n: usize = rng.gen_range(1..=3);
        for _ in 0..n {
            out.push(noise[rng.gen_range(0..noise.len())]);
        }
    }
    out
}

/// ¿El usuario debe resolver un captcha ahora? Gate previo al alta: IP nueva
/// y captcha habilitado. Compartido por ambos transports para que la
/// decisión sea idéntica.
pub fn needs_captcha_now(ctx: &AppContext, ip: IpAddr) -> bool {
    ctx.settings.security.captcha_enabled
        && !ctx.is_local_host(ip)
        && !ctx.user_history.has_prior_join(ip)
}

/// ¿El usuario tiene un captcha pendiente? (comprobación centralizada).
pub fn has_pending_captcha(user: &AresUser) -> bool {
    user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed)
}

/// ¿El usuario está en cuarentena?
pub fn is_quarantined(user: &AresUser) -> bool {
    user.quarantined.load(std::sync::atomic::Ordering::Relaxed)
}

/// Dispara el evento `Rejected` de scripting con la razón dada.
pub fn dispatch_rejected(scripting: &astra_scripting::ScriptHandle, name: &str, ip: IpAddr, reason: &str) {
    scripting.dispatch(astra_scripting::ScriptEvent::Rejected {
        name: name.to_string(),
        ip: ip.to_string(),
        reason: reason.to_string(),
    });
}

/// Dispara `InvalidLoginAttempt` + stats (helper para ambos transports).
pub fn on_invalid_login(
    ctx: &AppContext,
    scripting: &astra_scripting::ScriptHandle,
    name: &str,
    ip: IpAddr,
) {
    ctx.stats.on_invalid_login();
    scripting.dispatch(astra_scripting::ScriptEvent::InvalidLoginAttempt {
        name: name.to_string(),
        ip: ip.to_string(),
    });
}

/// Nivel mínimo requerido para el comando `/vpncheck`.
pub const VPNCHECK_LEVEL: ILevel = ILevel::Admin;

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;
    use std::sync::Arc;

    fn test_ctx() -> Arc<AppContext> {
        Arc::new(AppContext::new(
            Settings::default(),
            Database::in_memory().unwrap(),
        ))
    }

    #[test]
    fn obfuscated_word_contains_original_chars_in_order() {
        let word = "ABCD";
        let out = obfuscate_captcha_word(word);
        // La palabra debe poder leerse en orden dentro del ruido: cada letra
        // original va seguida de 1..=3 chars de ruido.
        let noise = ['#', '*', '.', '-', '+', '~', '=', '^'];
        let mut it = out.chars();
        for c in word.chars() {
            assert_eq!(it.next(), Some(c), "letra esperada {c}");
            // Consumir el ruido que sigue hasta la próxima letra.
            let mut consumed = 0;
            while let Some(next) = it.clone().next() {
                if !noise.contains(&next) {
                    break;
                }
                it.next();
                consumed += 1;
            }
            assert!((1..=3).contains(&consumed), "ruido {consumed} fuera de rango");
        }
    }

    #[test]
    fn captcha_prompt_has_placeholder_and_bot_name() {
        let ctx = test_ctx();
        let p = captcha_prompt(&ctx);
        assert!(p.contains("{{code}}"));
        assert!(p.contains(&ctx.settings.bot_name));
    }

    fn login(name: &str) -> server_core::login::LoginData {
        use std::net::Ipv4Addr;
        server_core::login::LoginData {
            guid: [0x42; 16],
            file_count: 0,
            crypto: false,
            data_port: 1234,
            node_ip: Ipv4Addr::UNSPECIFIED,
            node_port: 0,
            org_name: name.to_string(),
            version: "Ares 2.1.0".to_string(),
            is_ares: true,
            is_cbot: false,
            local_ip: Ipv4Addr::UNSPECIFIED,
            browsable: false,
            current_uploads: 0,
            max_uploads: 0,
            current_queued: 0,
            age: 25,
            sex: 1,
            country: 49,
            region: "US".to_string(),
            voice_chat_public: false,
            voice_chat_private: false,
            voice_opus_chat_public: false,
            voice_opus_chat_private: false,
            supports_html: false,
        }
    }

    #[test]
    fn evaluate_allows_clean_login() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        let l = login("Alice");
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(matches!(
            evaluate(&ctx, &l, ip, &sh),
            Admission::Allow { hijacked: false }
        ));
    }

    #[test]
    fn evaluate_rejects_join_filter() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        ctx.join_filters.add("Bad*");
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        match evaluate(&ctx, &login("BadActor"), ip, &sh) {
            Admission::Reject { kind, .. } => {
                assert_eq!(kind, server_core::security::RejectReason::InvalidName)
            }
            other => panic!("esperaba Reject, fue {other:?}"),
        }
    }

    #[test]
    fn evaluate_rejects_range_ban() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        ctx.range_bans.add("203.0.113");
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(matches!(
            evaluate(&ctx, &login("Alice"), ip, &sh),
            Admission::Reject { .. }
        ));
    }

    #[test]
    fn evaluate_rejects_vpn_when_action_reject() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        ctx.vpn_filter.add(server_core::VpnBlockKind::Cidr, "198.51.100.0/24");
        ctx.vpn_filter.set_enabled(true);
        ctx.vpn_filter.set_action(server_core::VpnAction::Reject);
        let ip: IpAddr = "198.51.100.7".parse().unwrap();
        match evaluate(&ctx, &login("Alice"), ip, &sh) {
            Admission::Reject { kind, .. } => {
                assert_eq!(kind, server_core::security::RejectReason::VpnBlocked)
            }
            other => panic!("esperaba Reject, fue {other:?}"),
        }
    }

    #[test]
    fn evaluate_vpn_report_lets_through() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        ctx.vpn_filter.add(server_core::VpnBlockKind::Cidr, "198.51.100.0/24");
        ctx.vpn_filter.set_enabled(true);
        ctx.vpn_filter.set_action(server_core::VpnAction::Report);
        let ip: IpAddr = "198.51.100.7".parse().unwrap();
        assert!(matches!(
            evaluate(&ctx, &login("Alice"), ip, &sh),
            Admission::Allow { .. }
        ));
    }

    #[test]
    fn evaluate_vpn_captcha_action_returns_captcha() {
        let ctx = test_ctx();
        let sh = astra_scripting::ScriptHandle::dummy();
        ctx.vpn_filter.add(server_core::VpnBlockKind::Cidr, "198.51.100.0/24");
        ctx.vpn_filter.set_enabled(true);
        ctx.vpn_filter.set_action(server_core::VpnAction::Captcha);
        let ip: IpAddr = "198.51.100.7".parse().unwrap();
        assert!(matches!(
            evaluate(&ctx, &login("Alice"), ip, &sh),
            Admission::Captcha { .. }
        ));
    }

    #[test]
    fn issue_and_verify_captcha_roundtrip() {
        let ctx = test_ctx();
        let user = AresUser::new(7, "1.2.3.4".parse().unwrap(), [0x11; 16]);
        let prompt = issue_captcha(&ctx, &user);
        assert!(has_pending_captcha(&user));
        assert!(is_quarantined(&user));
        // La palabra real no está en el prompt (está ofuscada); para el test
        // tomamos el challenge pendiente vía el manager.
        let _ = prompt;
        // Verificamos con la palabra interna creando un challenge conocido:
        // no la exponemos, así que probamos el camino de respuesta incorrecta.
        let bad = verify_captcha(&ctx, &user, "ZZZZ-no-coincide");
        assert!(matches!(bad, CaptchaOutcome::Wrong { .. } | CaptchaOutcome::Failed));
    }
}
