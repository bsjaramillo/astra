//! HTML del panel de administración web (`GET /admin`).
//!
//! Antes vivía aquí también un `INDEX_HTML` con un chat de prueba que se
//! servía en cualquier GET al puerto web y hacía auto-login como "WebUser":
//! abrir la URL de la sala en el navegador te metía dentro. Se eliminó
//! (2026-08-07); ese GET ahora responde una línea de texto plano.

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
///
/// El markup, los estilos y el JS viven en archivos reales bajo
/// `crates/web/panel/` (embebidos en el binario con `include_str!`, sin build
/// del frontend).
pub const ADMIN_HTML: &str = include_str!("../panel/index.html");
/// Hoja de estilos del panel (`GET /admin/style.css`).
pub const ADMIN_CSS: &str = include_str!("../panel/style.css");
/// JavaScript del panel (`GET /admin/app.js`).
pub const ADMIN_JS: &str = include_str!("../panel/app.js");

// ── CodeMirror 5 (editor de scripts) ──────────────────────────────────────
// Vendorizado en `crates/web/panel/vendor/` (MIT, ver LICENSE.txt) y servido
// como assets estáticos sin token, igual que `style.css`/`app.js`.
/// Núcleo de CodeMirror (`GET /admin/vendor/codemirror.js`).
pub const ADMIN_CM_JS: &str = include_str!("../panel/vendor/codemirror.min.js");
/// Estilos base de CodeMirror (`GET /admin/vendor/codemirror.css`).
pub const ADMIN_CM_CSS: &str = include_str!("../panel/vendor/codemirror.min.css");
/// Modo JavaScript/JSON (`GET /admin/vendor/mode/javascript.js`).
pub const ADMIN_CM_MODE_JS: &str = include_str!("../panel/vendor/javascript.min.js");
/// Modo XML/HTML (`GET /admin/vendor/mode/xml.js`).
pub const ADMIN_CM_MODE_XML: &str = include_str!("../panel/vendor/xml.min.js");
/// Modo CSS (`GET /admin/vendor/mode/css.js`).
pub const ADMIN_CM_MODE_CSS: &str = include_str!("../panel/vendor/css.min.js");
/// Modo Markdown (`GET /admin/vendor/mode/markdown.js`).
pub const ADMIN_CM_MODE_MD: &str = include_str!("../panel/vendor/markdown.min.js");
/// Addon de cierre automático de paréntesis (`GET /admin/vendor/addon/closebrackets.js`).
pub const ADMIN_CM_ADDON_CLOSEBRACKETS: &str =
    include_str!("../panel/vendor/closebrackets.min.js");
/// Addon de resaltado de paréntesis (`GET /admin/vendor/addon/matchbrackets.js`).
pub const ADMIN_CM_ADDON_MATCHBRACKETS: &str =
    include_str!("../panel/vendor/matchbrackets.min.js");
