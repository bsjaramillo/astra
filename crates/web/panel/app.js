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
    nav_motd:"Mensaje de entrada", nav_plantillas:"Textos del sistema", nav_bot:"Bot agente", nav_scripts:"Scripts",
    common_save:"Guardar", common_save_changes:"Guardar cambios", common_add:"Agregar", common_remove:"Quitar",
    common_none:"Ninguno.", common_none_f:"Ninguna.", common_loading:"Cargando…", common_error:"Error", common_done:"Listo", common_close:"Cerrar",
    restart_note:"⚠️ Estos cambios se guardan en el archivo de configuración y se aplican al <b>reiniciar el servidor</b>.",
    saved_restart:"Guardado. Reinicia el servidor para aplicar los cambios.",
    err_prefix:"Error: ", err_save:"no se pudo guardar",

    inicio_h:"Inicio", inicio_sub:"Estado general de tu sala, en tiempo real.",
    tile_room:"Sala", tile_bot:"Bot", tile_online:"En línea", tile_peak:"Pico",
    tile_total:"Ingresos totales", tile_bans:"Baneos activos", tile_uptime:"Tiempo activo",
    inicio_topic_h:"💬 Tema y estado", inicio_topic_l:"Tema de la sala (topic)",
    inicio_status_l:"Estado (mensaje corto)", inicio_status_ph:"ej. sala en mantenimiento",
    upd_avail:"Hay una nueva versión de Astra: v{0} (corriendo v{1}). Actualiza desde astra-creator.",
    upd_err:"No se pudo comprobar si hay actualizaciones (error del registry). Reintento cada hora; revisá que el server tenga salida a ghcr.io.",
    toast_topic:"Tema actualizado", toast_status:"Estado actualizado",

    users_h:"Usuarios en línea", users_sub:"{0} conectado(s). Toca una acción para moderar.",
    users_empty:"No hay nadie conectado en este momento.",
    u_muted:"silenciado", u_room:"sala", u_files:"archivos",
    u_info:"ℹ️ Info", u_kick:"👢 Expulsar", u_ban:"🚫 Banear", u_mute:"🔇 Silenciar", u_unmute:"🔊 Reactivar",
    u_changerank:"Cambiar rango…", u_to_voice:"→ Voz", u_to_mod:"→ Moderador", u_to_admin:"→ Administrador", u_remrank:"→ Quitar rango",
    u_more:"Más acciones",
    u_ban10:"🚫 Banear 10 min", u_ban60:"🚫 Banear 60 min", u_disableavatar:"🖼️ Quitar avatar",
    u_kiddie:"Kiddie", u_unkiddie:"Quitar kiddie",
    u_lower:"Minúsculas", u_unlower:"Quitar minúsculas",
    u_kewl:"Leetspeak", u_unkewl:"Quitar leetspeak",
    u_paint:"Pintar", u_unpaint:"Quitar pintura",
    u_echo_off:"Quitar heckle",
    u_id:"ID", u_oldname:"Nombre original", u_locate:"Ubicación", u_trace:"Rastrear",
    u_customname:"Nombre custom…", u_uncustomname:"Quitar nombre custom",
    u_autologin:"Auto-login…", u_autologin_mod:"→ Moderador", u_autologin_admin:"→ Admin", u_autologin_host:"→ Host",
    autologins_h:"🔑 Auto-login por IP", autologins_sub:"Restauran el rango automáticamente al reconocer la IP (sin contraseña).",
    autologins_empty:"No hay entradas de auto-login.",
    toast_autologin_added:"Auto-login otorgado: {0}", toast_autologin_rem:"Auto-login quitado",
    toast_customname:"Nombre custom actualizado", toast_echo_off:"Heckle quitado",
    prompt_customname:"Nombre custom para {0}:",
    cf_ban:"¿Seguro que quieres banear a {0}?",
    cf_ban10:"¿Banear a {0} por 10 minutos?", cf_ban60:"¿Banear a {0} por 60 minutos?", cf_kick:"¿Expulsar a {0}?",
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
    dir_h:"Directorio público de salas",
    dir_sub:"Publica tu sala para que se encuentre desde un navegador y se entre sin instalar nada.",
    dir_enabled:"Publicar esta sala en el directorio",
    dir_listed:"Aparecer en el listado",
    dir_listed_hint:"Desmárcalo para retirarla del listado sin dejar de anunciarla: así puedes volver a publicarla desde aquí.",
    dir_desc:"Descripción",
    dir_desc_ph:"De qué va tu sala, en una línea",
    dir_tags:"Etiquetas",
    dir_tags_ph:"musica, latino, rock",
    dir_tags_hint:"Separadas por comas. Solo valen: musica, latino, rock, chill, juegos, anime, deportes, amistad, ayuda, programacion, cine, 18",
    dir_website:"Web de la sala",
    dir_host:"Dominio propio (opcional)",
    dir_host_hint:"Si tu sala tiene dominio con HTTPS, ponlo aquí y marca la casilla: el cliente web usará wss:// en vez de ws://.",
    dir_tls:"La sala acepta conexiones seguras (wss://)",
    dir_url:"Tu sala está publicada en",
    dir_pending:"Aún sin publicar. Guarda los cambios y reinicia; la ficha aparece en unos minutos.",
    dir_privacy:"Nunca se envía el guid del servidor, la contraseña de dueño ni dato alguno de los usuarios conectados.",
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
    sec_spam_h:"🛡️ Anti-spam de mensajes", sec_spam_on:"Activar anti-spam",
    sec_spam_rate:"Limitar velocidad de mensajes", sec_spam_dup:"Detectar mensajes repetidos",
    sec_spam_maxmsgs:"Máx. mensajes de sala por ventana", sec_spam_maxpm:"Máx. mensajes privados por ventana",
    sec_spam_window:"Ventana (seg)", sec_spam_dupcount:"Repeticiones para marcar", sec_spam_minchars:"Largo mínimo a analizar",
    sec_spam_sim:"Similitud para duplicado (%)", sec_spam_action:"Acción",
    sec_spam_act_warn:"Solo avisar", sec_spam_act_mute:"Silenciar", sec_spam_act_kick:"Expulsar", sec_spam_act_ban:"Banear",
    sec_spam_mutesec:"Duración del silencio (seg)", sec_spam_bansesec:"Duración del ban (seg)",
    sec_spam_note:"Se aplica al instante (sin reiniciar). Exentos: Voz o superior.",

    proxy_h:"Proxies de confianza", proxy_sub:"Para cuando tu servidor está detrás de un proxy (Cloudflare, nginx, etc.).",
    proxy_note:"Solo las IP de esta lista pueden decir cuál es la IP real del visitante (vía cabeceras <code>X-Forwarded-For</code>/<code>X-Real-IP</code>). Aplica solo a clientes web. La IP local (127.0.0.1) siempre es de confianza. Los cambios se aplican al instante.",
    toast_proxy_add:"Proxy agregado", toast_proxy_rem:"Proxy quitado",
    nav_vpn:"Anti-VPN", vpn_h:"Filtro anti-VPN / proxy", vpn_sub:"Bloquea conexiones desde VPNs, proxies y datacenters conocidos.",
    vpn_note:"Combina la base ASN local y una blocklist CIDR. En modo <b>report</b> solo registra (ideal para medir falsos positivos antes de bloquear); <b>reject</b> cierra la conexión; <b>captcha</b> y <b>quarantine</b> dejan entrar pero exigen captcha o silencian. Los cambios se aplican al instante.",
    vpn_cfg_h:"Configuración", vpn_enabled:"Filtrar conexiones VPN/proxy", vpn_action:"Acción al detectar",
    vpn_refresh:"Refrescar feed cada (horas)", vpn_feed:"URL del feed (vacío = sin descarga)",
    vpn_entries_h:"Entradas", vpn_th_kind:"Tipo", vpn_th_value:"Valor", vpn_th_source:"Origen",
    vpn_clear_feed:"Vaciar feed", vpn_clear_manual:"Vaciar manuales",
    vpn_import_h:"Importar lista", vpn_import_sub:"Pegá una lista (una por línea): rangos CIDR o ASNs (<code>AS64500</code>). Reemplaza las entradas del feed, no las manuales.",
    vpn_import_btn:"Importar", vpn_act_report:"Solo reportar", vpn_act_reject:"Rechazar", vpn_act_captcha:"Pedir captcha", vpn_act_quarantine:"Cuarentena (silenciar)",
    vpn_saved:"Configuración guardada", vpn_added:"Entrada agregada", vpn_invalid:"Valor inválido", vpn_imported:"{0} entradas importadas",
    geoip_h:"Base ASN/GeoIP", geoip_sub:"Sin la base ASN no se puede resolver el ASN de una IP: la parte ASN del filtro y <code>/asnban</code> quedan inertes.",
    geoip_enabled:"Descargar/refrescar automáticamente", geoip_asn_url:"URL de la base ASN (admite {YYYY-MM})",
    geoip_city_url:"URL de la base de ciudad (opcional)", geoip_city_ph:"Vacío = no descargar ciudad",
    geoip_refresh:"Refrescar cada (horas)", geoip_refresh_now:"Actualizar ahora",
    geoip_asn_loaded:"ASN cargado", geoip_asn_missing:"ASN sin cargar",
    geoip_saved:"Configuración de GeoIP guardada", geoip_refresh_queued:"Actualización en curso…",
    vpn_refreshing:"Descargando feed…", vpn_count:"{0} entradas", vpn_empty:"Sin entradas",
    vpn_refresh_now:"Refrescar feed", vpn_refresh_queued:"Descarga del feed en curso…",
    vpn_count_hint:"Al activar el filtro, el feed se descarga de inmediato. El conteo se actualiza al terminar.",
    vpn_enforce_now:"Aplicar a conectados", vpn_enforced:"Aplicado: {0} expulsados, {1} en cuarentena",
    vpn_allow_h:"Exenciones (allowlist)", vpn_allow_sub:"IPs o rangos que nunca se bloquean, aunque aparezcan en la lista. Usalo para corregir falsos positivos.",
    vpn_allow_btn:"Permitir", vpn_allow_added:"Exención agregada", vpn_allow_exists:"Esa IP ya estaba permitida",
    vpn_det_h:"Detecciones recientes", vpn_det_sub:"Últimas conexiones bloqueadas o reportadas por el filtro. Si alguna es un falso positivo, tocá «Permitir» para eximirla.",
    vpn_det_th_ip:"IP", vpn_det_th_name:"Nick", vpn_det_th_rule:"Regla", vpn_det_th_action:"Acción", vpn_det_th_hits:"Intentos", vpn_det_clear:"Vaciar registro", vpn_det_cleared:"Registro vaciado",
    vpn_search_ph:"Buscar IP o ASN…", vpn_filter_all:"Todas", vpn_prev:"Anterior", vpn_next:"Siguiente", vpn_page_info:"Página {0} de {1} ({2})", vpn_per_page:"Filas por página",

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

    bot_h:"Bot agente", bot_sub:"Asistente inteligente con identidad propia (LLM).",
    bot_note:"Se aplica en vivo. El bot aparece en la lista de usuarios solo cuando está activo.",
    bot_enabled:"Activar bot", bot_name_l:"Nombre", bot_name_ph:"ej. Nova",
    bot_avatar_l:"Avatar (foto)", bot_avatar_clear:"Quitar avatar",
    bot_greet_h:"👋 Saludos al entrar", bot_greet_on:"Saludar a quien entra", bot_greet_pm:"Saludo por PM (si no, en sala)",
    bot_greet_llm:"Generar el saludo con el LLM",
    bot_greet_msg:"Mensaje de saludo (fallback si el LLM falla)", bot_greet_ph:"¡Hola +n! Bienvenido a +rn.",
    bot_reply_h:"💬 Conversación", bot_reply_room:"Responder menciones en sala", bot_reply_pm:"Responder PMs",
    bot_trigger:"Disparador", bot_trigger_contains:"Cuando mencionan su nombre", bot_trigger_prefix:"Cuando el mensaje empieza con", bot_trigger_always:"Responder a todo",
    bot_prefix_l:"Prefijo", bot_memory:"Recordar conversación", bot_memory_turns:"Turns de memoria",
    bot_history_lines:"Msgs. recientes de sala (0=off)",
    bot_cooldown:"Cooldown (seg)", bot_max_inflight:"Máx. llamadas simultáneas",
    bot_llm_h:"🤖 Proveedor LLM", bot_provider:"Proveedor", bot_provider_openai:"OpenAI", bot_provider_deepseek:"DeepSeek", bot_provider_anthropic:"Anthropic",
    bot_api_key:"API key", bot_api_key_req:"Para activar el bot, la API key del LLM es obligatoria.", bot_model:"Modelo", bot_temp:"Temperatura", bot_max_tokens:"Máx. tokens",
    bot_prompt:"Prompt de sistema (personalidad)", bot_prompt_ph:"Eres Nova, un asistente amable y cercano...",
    bot_fallback:"Respuesta si el LLM falla",
    bot_exec_h:"Ejecución de comandos", bot_exec_note:"El bot puede ejecutar comandos que el usuario le pida. Se ejecutan con el NIVEL del usuario que lo pide (un Regular no puede banear a un Admin). Apagado por defecto.",
    bot_exec_on:"Permitir ejecutar comandos", bot_allowed_cmds:"Comandos permitidos (vacío = todos por nivel, separados por coma)",
    bot_help:"¿Cómo configurar la API key?",
    bot_help_intro:"La API key es obligatoria para ACTIVAR el bot. El backend usa Rig con las URLs oficiales de cada proveedor: solo hay que elegir proveedor y modelo.",
    bot_help_provider:"Proveedor", bot_help_model:"Modelo default",
    bot_help_key:"Generar API key", bot_help_balance:"Saldo / recarga",
    bot_help_balance_openai:"Prepago: recarga en platform.openai.com → Billing.",
    bot_help_balance_deepseek:"Prepago obligatorio: recarga / top-up en platform.deepseek.com.",
    bot_help_balance_anthropic:"Créditos prepago: recarga en platform.anthropic.com → Billing.",
    bot_help_note:"Si el bot responde el mensaje de fallback, mirá el log «bot: error LLM para 'X': …». 401 = key inválida · 402 = sin saldo · 422 = modelo mal · 429/503/insufficient_system_resource = transitorio (se reintenta) · timeout → subir timeout_secs.",
    bot_saved:"Bot guardado.",
    bot_select:"Bot a editar", bot_none:"Sin bots — creá uno abajo", bot_new:"Nuevo bot", bot_del:"Eliminar bot",
    bot_del_confirm:"¿Eliminar este bot?", bot_deleted:"Bot eliminado.", bot_identity:"Identidad",

    sc_h:"Scripts de la sala", sc_sub:"Revisa, recarga e instala scripts (plugins JS) sin salir del panel.",
    sc_note:"Los scripts se cargan desde la carpeta <code>scripts/</code> del servidor. <b>Recargar</b> relee el archivo desde disco; <b>Descargar</b> solo lo quita de memoria (el archivo queda).",
    sc_installed_h:"📦 Instalados", sc_refresh:"Actualizar",
    sc_empty:"No hay scripts. Instala uno desde la comunidad o coloca una carpeta en <code>scripts/</code>.",
    sc_state_active:"Activo", sc_state_error:"Error", sc_state_loaded:"Cargado", sc_state_disk:"En disco", sc_state_unloaded:"Descargado",
    sc_view:"Ver código", sc_reload:"Recargar", sc_unload:"Descargar", sc_load:"Cargar",
    sc_folder:"carpeta", sc_file:"archivo", sc_files:"{0} archivo(s)",
    sc_source_title:"Código de {0}",
    sc_community_h:"🌐 Comunidad",
    sc_community_note:"Scripts públicos de GitHub con el topic <code>areschatscript</code>. Se instalan en <code>scripts/&lt;repo&gt;/</code> y se cargan solos.",
    sc_search_ph:"Buscar scripts (ej. trivia, moderación…)", sc_search_btn:"Buscar", sc_searching:"Buscando…",
    sc_no_results:"Sin resultados.", sc_search_err:"no se pudo buscar",
    sc_author:"por {0}", sc_stars:"★ {0}", sc_install:"Instalar", sc_installing:"Instalando…",
    sc_installed_ok:"Script instalado y cargado.", sc_install_err:"no se pudo instalar",
    sc_load_ok:"Script cargado.", sc_unload_ok:"Script descargado.",
    sc_kill_confirm:"¿Descargar el script '{0}' de memoria?", sc_error_label:"Error",

    nav_soporte:"Soporte",
    sup_h:"Soporte", sup_sub:"Reportá un problema o sugerí una mejora para Astra.",
    sup_note:"El reporte va al repositorio oficial de Astra. Solo se envía el título y la descripción que escribas (ningún dato del servidor ni de los usuarios).",
    sup_kind_label:"Tipo", sup_kind_bug:"Problema (bug)", sup_kind_idea:"Mejora / idea",
    sup_title_label:"Título", sup_title_ph:"Resumen corto del problema",
    sup_desc_label:"Descripción", sup_desc_ph:"Qué pasó, qué esperabas que pasara y cómo reproducirlo.",
    sup_open:"Abrir en GitHub", sup_send:"Enviar directamente",
    sup_open_hint:"Se abrirá GitHub con el reporte pre-rellenado; necesitás una cuenta para enviarlo.",
    sup_direct_hint:"El envío directo está disponible porque hay un token de GitHub configurado.",
    sup_no_token:"Para enviar sin cuenta de GitHub, configurá <code>[github] token</code> en astra.toml.",
    sup_title_required:"Escribí un título.",
    sup_sending:"Enviando…", sup_ok:"Reporte enviado:", sup_err:"no se pudo enviar",
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
    nav_motd:"Join message", nav_plantillas:"System texts", nav_bot:"Agent bot", nav_scripts:"Scripts",
    common_save:"Save", common_save_changes:"Save changes", common_add:"Add", common_remove:"Remove",
    common_none:"None.", common_none_f:"None.", common_loading:"Loading…", common_error:"Error", common_done:"Done", common_close:"Close",
    restart_note:"⚠️ These changes are written to the config file and take effect after <b>restarting the server</b>.",
    saved_restart:"Saved. Restart the server to apply the changes.",
    err_prefix:"Error: ", err_save:"couldn't save",

    inicio_h:"Home", inicio_sub:"An overview of your room, in real time.",
    tile_room:"Room", tile_bot:"Bot", tile_online:"Online", tile_peak:"Peak",
    tile_total:"Total joins", tile_bans:"Active bans", tile_uptime:"Uptime",
    inicio_topic_h:"💬 Topic & status", inicio_topic_l:"Room topic",
    inicio_status_l:"Status (short message)", inicio_status_ph:"e.g. room under maintenance",
    upd_avail:"A new Astra version is available: v{0} (running v{1}). Update from astra-creator.",
    upd_err:"Couldn't check for updates (registry error). Will retry hourly; make sure this server can reach ghcr.io.",
    toast_topic:"Topic updated", toast_status:"Status updated",

    users_h:"Users online", users_sub:"{0} connected. Tap an action to moderate.",
    users_empty:"Nobody is connected right now.",
    u_muted:"muted", u_room:"room", u_files:"files",
    u_info:"ℹ️ Info", u_kick:"👢 Kick", u_ban:"🚫 Ban", u_mute:"🔇 Mute", u_unmute:"🔊 Unmute",
    u_changerank:"Change rank…", u_to_voice:"→ Voice", u_to_mod:"→ Moderator", u_to_admin:"→ Administrator", u_remrank:"→ Remove rank",
    u_more:"More actions",
    u_ban10:"🚫 Ban 10 min", u_ban60:"🚫 Ban 60 min", u_disableavatar:"🖼️ Remove avatar",
    u_kiddie:"Kiddie", u_unkiddie:"Un-kiddie",
    u_lower:"Lowercase", u_unlower:"Un-lowercase",
    u_kewl:"Leetspeak", u_unkewl:"Remove leetspeak",
    u_paint:"Paint", u_unpaint:"Remove paint",
    u_echo_off:"Stop heckle",
    u_id:"ID", u_oldname:"Original name", u_locate:"Location", u_trace:"Trace",
    u_customname:"Custom name…", u_uncustomname:"Clear custom name",
    u_autologin:"Auto-login…", u_autologin_mod:"→ Moderator", u_autologin_admin:"→ Admin", u_autologin_host:"→ Host",
    autologins_h:"🔑 IP auto-login", autologins_sub:"Restore the rank automatically when the IP is recognized (no password).",
    autologins_empty:"No auto-login entries.",
    toast_autologin_added:"Auto-login granted: {0}", toast_autologin_rem:"Auto-login removed",
    toast_customname:"Custom name set", toast_echo_off:"Heckle cleared",
    prompt_customname:"Custom name for {0}:",
    cf_ban:"Ban {0}?",
    cf_ban10:"Ban {0} for 10 minutes?", cf_ban60:"Ban {0} for 60 minutes?", cf_kick:"Kick {0}?",
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
    dir_h:"Public room directory",
    dir_sub:"Publish your room so people can find it from a browser and join without installing anything.",
    dir_enabled:"Publish this room in the directory",
    dir_listed:"Show in the listing",
    dir_listed_hint:"Uncheck to pull it from the listing while still announcing, so you can publish it again from here.",
    dir_desc:"Description",
    dir_desc_ph:"What your room is about, in one line",
    dir_tags:"Tags",
    dir_tags_ph:"musica, latino, rock",
    dir_tags_hint:"Comma separated. Only these are accepted: musica, latino, rock, chill, juegos, anime, deportes, amistad, ayuda, programacion, cine, 18",
    dir_website:"Room website",
    dir_host:"Own domain (optional)",
    dir_host_hint:"If your room has a domain with HTTPS, put it here and tick the box: the web client will use wss:// instead of ws://.",
    dir_tls:"The room accepts secure connections (wss://)",
    dir_url:"Your room is published at",
    dir_pending:"Not published yet. Save and restart; the listing shows up within minutes.",
    dir_privacy:"The server guid, the owner password and any data about connected users are never sent.",
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
    sec_spam_h:"🛡️ Message anti-spam", sec_spam_on:"Enable anti-spam",
    sec_spam_rate:"Rate-limit messages", sec_spam_dup:"Detect repeated messages",
    sec_spam_maxmsgs:"Max room messages per window", sec_spam_maxpm:"Max PMs per window",
    sec_spam_window:"Window (sec)", sec_spam_dupcount:"Repeats to flag", sec_spam_minchars:"Min length to analyze",
    sec_spam_sim:"Duplicate similarity (%)", sec_spam_action:"Action",
    sec_spam_act_warn:"Warn only", sec_spam_act_mute:"Mute", sec_spam_act_kick:"Kick", sec_spam_act_ban:"Ban",
    sec_spam_mutesec:"Mute duration (sec)", sec_spam_bansesec:"Ban duration (sec)",
    sec_spam_note:"Applies instantly (no restart). Exempt: Voice or higher.",

    proxy_h:"Trusted proxies", proxy_sub:"For when your server sits behind a proxy (Cloudflare, nginx, etc.).",
    proxy_note:"Only IPs on this list may report the visitor's real IP (via <code>X-Forwarded-For</code>/<code>X-Real-IP</code> headers). Applies to web clients only. Localhost (127.0.0.1) is always trusted. Changes apply instantly.",
    toast_proxy_add:"Proxy added", toast_proxy_rem:"Proxy removed",
    nav_vpn:"Anti-VPN", vpn_h:"Anti-VPN / proxy filter", vpn_sub:"Blocks connections from known VPNs, proxies and datacenters.",
    vpn_note:"Combines the local ASN database and a CIDR blocklist. In <b>report</b> mode it only logs (ideal to measure false positives before blocking); <b>reject</b> closes the connection; <b>captcha</b> and <b>quarantine</b> let them in but require a captcha or mute them. Changes apply instantly.",
    vpn_cfg_h:"Settings", vpn_enabled:"Filter VPN/proxy connections", vpn_action:"Action on detection",
    vpn_refresh:"Refresh feed every (hours)", vpn_feed:"Feed URL (empty = no download)",
    vpn_entries_h:"Entries", vpn_th_kind:"Kind", vpn_th_value:"Value", vpn_th_source:"Source",
    vpn_clear_feed:"Clear feed", vpn_clear_manual:"Clear manual",
    vpn_import_h:"Import list", vpn_import_sub:"Paste a list (one per line): CIDR ranges or ASNs (<code>AS64500</code>). Replaces feed entries, not manual ones.",
    vpn_import_btn:"Import", vpn_act_report:"Report only", vpn_act_reject:"Reject", vpn_act_captcha:"Require captcha", vpn_act_quarantine:"Quarantine (mute)",
    vpn_saved:"Settings saved", vpn_added:"Entry added", vpn_invalid:"Invalid value", vpn_imported:"{0} entries imported",
    geoip_h:"ASN/GeoIP database", geoip_sub:"Without the ASN database Astra can't resolve a IP's ASN: the ASN part of the filter and <code>/asnban</code> stay inert.",
    geoip_enabled:"Download/refresh automatically", geoip_asn_url:"ASN database URL (supports {YYYY-MM})",
    geoip_city_url:"City database URL (optional)", geoip_city_ph:"Empty = don't download city",
    geoip_refresh:"Refresh every (hours)", geoip_refresh_now:"Update now",
    geoip_asn_loaded:"ASN loaded", geoip_asn_missing:"ASN not loaded",
    geoip_saved:"GeoIP settings saved", geoip_refresh_queued:"Update in progress…",
    vpn_refreshing:"Downloading feed…", vpn_count:"{0} entries", vpn_empty:"No entries",
    vpn_refresh_now:"Refresh feed", vpn_refresh_queued:"Feed download in progress…",
    vpn_count_hint:"When you enable the filter, the feed downloads immediately. The count updates when it finishes.",
    vpn_enforce_now:"Apply to connected", vpn_enforced:"Applied: {0} kicked, {1} quarantined",
    vpn_allow_h:"Exemptions (allowlist)", vpn_allow_sub:"IPs or ranges that are never blocked, even if they appear on the list. Use it to fix false positives.",
    vpn_allow_btn:"Allow", vpn_allow_added:"Exemption added", vpn_allow_exists:"That IP was already allowed",
    vpn_det_h:"Recent detections", vpn_det_sub:"Latest connections blocked or reported by the filter. If one is a false positive, tap 'Allow' to exempt it.",
    vpn_det_th_ip:"IP", vpn_det_th_name:"Nick", vpn_det_th_rule:"Rule", vpn_det_th_action:"Action", vpn_det_th_hits:"Attempts", vpn_det_clear:"Clear log", vpn_det_cleared:"Log cleared",
    vpn_search_ph:"Search IP or ASN…", vpn_filter_all:"All", vpn_prev:"Previous", vpn_next:"Next", vpn_page_info:"Page {0} of {1} ({2})", vpn_per_page:"Rows per page",

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

    bot_h:"Agent bot", bot_sub:"Intelligent assistant with its own identity (LLM).",
    bot_note:"Applied live. The bot shows in the user list only while active.",
    bot_enabled:"Enable bot", bot_name_l:"Name", bot_name_ph:"e.g. Nova",
    bot_avatar_l:"Avatar (photo)", bot_avatar_clear:"Remove avatar",
    bot_greet_h:"👋 Join greetings", bot_greet_on:"Greet users on join", bot_greet_pm:"Greet by PM (otherwise in room)",
    bot_greet_llm:"Generate the greeting with the LLM",
    bot_greet_msg:"Greeting message (fallback if the LLM fails)", bot_greet_ph:"Welcome +n to +rn!",
    bot_reply_h:"💬 Conversation", bot_reply_room:"Reply to mentions in room", bot_reply_pm:"Reply to PMs",
    bot_trigger:"Trigger", bot_trigger_contains:"When they mention its name", bot_trigger_prefix:"When the message starts with", bot_trigger_always:"Reply to everything",
    bot_prefix_l:"Prefix", bot_memory:"Remember conversation", bot_memory_turns:"Memory turns",
    bot_history_lines:"Recent room msgs (0=off)",
    bot_cooldown:"Cooldown (sec)", bot_max_inflight:"Max concurrent calls",
    bot_llm_h:"🤖 LLM provider", bot_provider:"Provider", bot_provider_openai:"OpenAI", bot_provider_deepseek:"DeepSeek", bot_provider_anthropic:"Anthropic",
    bot_api_key:"API key", bot_api_key_req:"To enable the bot, the LLM API key is required.", bot_model:"Model", bot_temp:"Temperature", bot_max_tokens:"Max tokens",
    bot_prompt:"System prompt (personality)", bot_prompt_ph:"You are Nova, a friendly assistant...",
    bot_fallback:"Reply if the LLM fails",
    bot_exec_h:"Command execution", bot_exec_note:"The bot can run commands users ask for. They run with the REQUESTING user's level (a Regular can't ban an Admin). Off by default.",
    bot_exec_on:"Allow executing commands", bot_allowed_cmds:"Allowed commands (blank = all by level, comma separated)",
    bot_help:"How to configure the API key?",
    bot_help_intro:"The API key is required to ENABLE the bot. The backend uses Rig with each provider's official URLs: just pick provider and model.",
    bot_help_provider:"Provider", bot_help_model:"Default model",
    bot_help_key:"Generate API key", bot_help_balance:"Balance / top-up",
    bot_help_balance_openai:"Prepaid: top up at platform.openai.com → Billing.",
    bot_help_balance_deepseek:"Prepaid (required): top up at platform.deepseek.com.",
    bot_help_balance_anthropic:"Prepaid credits: top up at platform.anthropic.com → Billing.",
    bot_help_note:"If the bot replies with the fallback, check the log «bot: error LLM para 'X': …». 401 = invalid key · 402 = no balance · 422 = wrong model · 429/503/insufficient_system_resource = transient (retried) · timeout → raise timeout_secs.",
    bot_saved:"Bot saved.",
    bot_select:"Bot to edit", bot_none:"No bots — create one below", bot_new:"New bot", bot_del:"Delete bot",
    bot_del_confirm:"Delete this bot?", bot_deleted:"Bot deleted.", bot_identity:"Identity",

    sc_h:"Room scripts", sc_sub:"Review, reload and install scripts (JS plugins) without leaving the panel.",
    sc_note:"Scripts load from the server's <code>scripts/</code> folder. <b>Reload</b> re-reads the file from disk; <b>Unload</b> only removes it from memory (the file stays).",
    sc_installed_h:"📦 Installed", sc_refresh:"Refresh",
    sc_empty:"No scripts. Install one from the community or drop a folder in <code>scripts/</code>.",
    sc_state_active:"Active", sc_state_error:"Error", sc_state_loaded:"Loaded", sc_state_disk:"On disk", sc_state_unloaded:"Unloaded",
    sc_view:"View code", sc_reload:"Reload", sc_unload:"Unload", sc_load:"Load",
    sc_folder:"folder", sc_file:"file", sc_files:"{0} file(s)",
    sc_source_title:"Source of {0}",
    sc_community_h:"🌐 Community",
    sc_community_note:"Public GitHub scripts tagged with <code>areschatscript</code>. They install to <code>scripts/&lt;repo&gt;/</code> and load automatically.",
    sc_search_ph:"Search scripts (e.g. trivia, moderation…)", sc_search_btn:"Search", sc_searching:"Searching…",
    sc_no_results:"No results.", sc_search_err:"search failed",
    sc_author:"by {0}", sc_stars:"★ {0}", sc_install:"Install", sc_installing:"Installing…",
    sc_installed_ok:"Script installed and loaded.", sc_install_err:"could not install",
    sc_load_ok:"Script loaded.", sc_unload_ok:"Script unloaded.",
    sc_kill_confirm:"Unload script '{0}' from memory?", sc_error_label:"Error",

    nav_soporte:"Support",
    sup_h:"Support", sup_sub:"Report a problem or suggest an improvement for Astra.",
    sup_note:"The report goes to the official Astra repository. Only the title and description you write are sent (no server or user data).",
    sup_kind_label:"Type", sup_kind_bug:"Problem (bug)", sup_kind_idea:"Improvement / idea",
    sup_title_label:"Title", sup_title_ph:"Short summary of the problem",
    sup_desc_label:"Description", sup_desc_ph:"What happened, what you expected, and how to reproduce it.",
    sup_open:"Open in GitHub", sup_send:"Send directly",
    sup_open_hint:"GitHub will open with the report pre-filled; you need an account to submit it.",
    sup_direct_hint:"Direct sending is available because a GitHub token is configured.",
    sup_no_token:"To send without a GitHub account, set <code>[github] token</code> in astra.toml.",
    sup_title_required:"Please write a title.",
    sup_sending:"Sending…", sup_ok:"Report sent:", sup_err:"could not send",
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
  es:{block:"Bloquear",kick:"Expulsar",ban:"Banear",muzzle:"Silenciar",announce:"Anunciar"},
  en:{block:"Block",kick:"Kick",ban:"Ban",muzzle:"Muzzle",announce:"Announce"}
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
    {id:"bot", icon:"🤖", k:"nav_bot"},
  ]},
  {gk:"g_avanzado", items:[
    {id:"servidor", icon:"🖥️", k:"nav_servidor"},
    {id:"enlace", icon:"🔗", k:"nav_enlace"},
    {id:"seguridad", icon:"🛡️", k:"nav_seguridad"},
    {id:"proxies", icon:"🌐", k:"nav_proxies"},
    {id:"vpn", icon:"🕵️", k:"nav_vpn"},
    {id:"permisos", icon:"🔑", k:"nav_permisos"},
    {id:"scripts", icon:"📜", k:"nav_scripts"},
    {id:"plantillas", icon:"💬", k:"nav_plantillas"},
    {id:"config", icon:"📝", k:"nav_config"},
    {id:"consola", icon:"⌨️", k:"nav_consola"},
    // Reporte a GitHub: OCULTO por ahora (el módulo sigue en el código).
    // Para mostrarlo, descomentar la línea de abajo y reiniciar el server.
    // {id:"soporte", icon:"🛟", k:"nav_soporte"},
  ]},
];
// Pestañas que NO se auto-refrescan (tienen formularios que se borrarían al
// re-renderizar mientras el admin escribe).
const STATIC = new Set(["consola","config","servidor","enlace","seguridad","permisos","proxies","vpn","avatares","motd","plantillas","bot","soporte","scripts"]);

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

function showOutput(title, lines){
  document.getElementById("modalTitle").textContent = title || "";
  document.getElementById("modalBody").textContent = (lines || []).join("\n") || t("common_none");
  document.getElementById("modal").classList.remove("hidden");
}
function closeModal(){ document.getElementById("modal").classList.add("hidden"); }

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
    enlace:renderEnlace, seguridad:renderSeguridad, proxies:renderProxies, vpn:renderVpn,
    permisos:renderPermisos, plantillas:renderPlantillas, config:renderConfig, consola:renderConsola,
    bot:renderBot, soporte:renderSoporte, scripts:renderScripts
  };
  // El auto-refresh re-renderiza el DOM y los <details> perderían su estado
  // abierto. Guardamos cuáles estaban abiertos y lo restauramos tras render.
  const open = [...document.querySelectorAll("details[data-more][open]")].map(d=>d.dataset.more);
  document.getElementById("view").innerHTML = (map[TAB] || renderInicio)();
  if(open.length) document.querySelectorAll("details[data-more]").forEach(d=>{
    if(open.includes(d.dataset.more)) d.open=true;
  });
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
  const upd = s.update ? `<div class="warnbox">🚀 ${t("upd_avail", s.update, s.version)}</div>`
    : s.updateError ? `<div class="warnbox">⚠️ ${t("upd_err")}</div>` : "";
  return `<div class="cardhead"><h2>${t("inicio_h")}</h2><p class="sub">${t("inicio_sub")}</p></div>
    ${upd}
    <div class="tiles">${tiles.map(x=>`<div class="tile"><span class="tl">${x[0]}</span><span class="tv">${x[1]}</span></div>`).join("")}</div>
    <div class="card"><h3>${t("inicio_topic_h")}</h3>
      <label class="fld"><span>${t("inicio_topic_l")}</span><div class="inline"><input id="topicIn" value="${esc(s.topic)}"><button class="btn primary" id="topicSet">${t("common_save")}</button></div></label>
      <label class="fld" style="margin-bottom:0"><span>${t("inicio_status_l")}</span><div class="inline"><input id="statusIn" value="${esc(s.status)}" placeholder="${t("inicio_status_ph")}"><button class="btn" id="statusSet">${t("common_save")}</button></div></label>
    </div>`;
}

function renderUsuarios(){
  const us = STATE.users||[];
  const cards = us.map(u=>{
    const n = esc(u.name);
    const muzAct = u.muzzled ? "unmuzzle" : "muzzle";
    const muzLbl = u.muzzled ? t("u_unmute") : t("u_mute");
    const kidAct = u.kiddied ? "unkiddy" : "kiddy";
    const lowAct = u.lowered ? "unlower" : "lower";
    const kewlAct = u.kewl ? "remkewltext" : "kewltext";
    const paintAct = u.painted ? "unpaint" : "paint";
    const more = [
      `<button class="btn sm danger" data-act="ban10" data-n="${n}">${t("u_ban10")}</button>`,
      `<button class="btn sm danger" data-act="ban60" data-n="${n}">${t("u_ban60")}</button>`,
      `<button class="btn sm" data-act="disableavatar" data-n="${n}">${t("u_disableavatar")}</button>`,
      `<button class="btn sm" data-act="${kidAct}" data-n="${n}">${u.kiddied?t("u_unkiddie"):t("u_kiddie")}</button>`,
      `<button class="btn sm" data-act="${lowAct}" data-n="${n}">${u.lowered?t("u_unlower"):t("u_lower")}</button>`,
      `<button class="btn sm" data-act="${kewlAct}" data-n="${n}">${u.kewl?t("u_unkewl"):t("u_kewl")}</button>`,
      `<button class="btn sm" data-act="${paintAct}" data-n="${n}">${u.painted?t("u_unpaint"):t("u_paint")}</button>`,
      u.echo?`<button class="btn sm" data-act="unecho" data-n="${n}">${t("u_echo_off")}</button>`:'',
      `<button class="btn sm" data-act="id" data-n="${n}">${t("u_id")}</button>`,
      `<button class="btn sm" data-act="oldname" data-n="${n}">${t("u_oldname")}</button>`,
      `<button class="btn sm" data-act="locate" data-n="${n}">${t("u_locate")}</button>`,
      `<button class="btn sm" data-act="trace" data-n="${n}">${t("u_trace")}</button>`,
      `<button class="btn sm" data-act="customname" data-n="${n}">${t("u_customname")}</button>`,
      u.custom?`<button class="btn sm" data-act="uncustomname" data-n="${n}">${t("u_uncustomname")}</button>`:'',
    ].join("");
    return `<div class="ucard">
      <div class="uhead"><span class="badge ${lvlClass(u.level)}">${esc(lvlName(u.levelName))}</span>
        <b class="uname">${n}</b>
        ${u.muzzled?`<span class="chip warn">${t("u_muted")}</span>`:''}</div>
      <div class="umeta">${esc(u.ip)} · ${t("u_room")} ${u.vroom} · ${u.files||0} ${t("u_files")}${u.version?` · <span class="mut">${esc(u.version)}</span>`:''}</div>
      <div class="uactions">
        <button class="btn sm" data-act="whois" data-n="${n}">${t("u_info")}</button>
        <button class="btn sm" data-act="kick" data-n="${n}">${t("u_kick")}</button>
        <button class="btn sm danger" data-act="ban" data-n="${n}">${t("u_ban")}</button>
        <button class="btn sm" data-act="${muzAct}" data-n="${n}">${muzLbl}</button>
        <select class="sel sm" data-grant="${n}">
          <option value="">${t("u_changerank")}</option>
          <option value="voice">${t("u_to_voice")}</option>
          <option value="moderator">${t("u_to_mod")}</option>
          <option value="admin">${t("u_to_admin")}</option>
          <option value="revoke">${t("u_remrank")}</option>
        </select>
      </div>
      <details class="umore" data-more="${n}"><summary>${t("u_more")}</summary>
        <div class="uactions">${more}</div>
        <div class="uflow"><label>${t("u_autologin")}</label>
          <select class="sel sm" data-autologin="${n}">
            <option value="">${t("u_autologin")}</option>
            <option value="1">${t("u_autologin_mod")}</option>
            <option value="2">${t("u_autologin_admin")}</option>
            <option value="3">${t("u_autologin_host")}</option>
          </select>
        </div>
      </details></div>`;
  }).join("");
  return `<div class="cardhead"><h2>${t("users_h")}</h2><p class="sub">${t("users_sub", us.length)}</p></div>
    <div class="ucards">${cards||`<div class="empty">${t("users_empty")}</div>`}</div>
    ${renderAutologins()}`;
}

function renderAutologins(){
  const al = STATE.autologins||[];
  const rows = al.map(a=>`<tr><td>${esc(a.name)}</td><td class="mut">${esc(a.ip)}</td>
    <td><span class="chip">${esc(lvlName(a.levelName))}</span></td>
    <td style="text-align:right"><button class="btn sm danger" data-remautologin="${a.id}">${t("common_remove")}</button></td></tr>`).join("");
  return `<div class="card"><h3>${t("autologins_h")}</h3>
    <p class="sub" style="margin-bottom:10px">${t("autologins_sub")}</p>
    <div class="scroll"><table class="tbl"><thead><tr><th>${t("th_name")}</th><th>IP</th><th>${t("th_rank")}</th><th></th></tr></thead>
    <tbody>${rows||`<tr><td colspan=4 class=mut>${t("autologins_empty")}</td></tr>`}</tbody></table></div></div>`;
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
    <div class="cardgrid">
    <div class="card"><h3>${t("bans_range_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("bans_range_desc")}</p>
      <div>${rb||`<span class=mut>${t("common_none")}</span>`}</div>
      <div class="inline" style="margin-top:10px"><input id="rbIn" placeholder="1.2.3."><button class="btn" id="rbAdd">${t("common_add")}</button></div></div>
    <div class="card"><h3>${t("bans_asn_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("bans_asn_desc")}</p>
      <div>${ab||`<span class=mut>${t("common_none")}</span>`}</div>
      <div class="inline" style="margin-top:10px"><input id="abIn" placeholder="${t("bans_asn_ph")}"><button class="btn" id="abAdd">${t("common_add")}</button></div></div>
    </div>`;
}

function renderFiltros(){
  const on = STATE.filtersEnabled!==false;
  const f = (STATE.filters||[]).map((x,i)=>`<tr><td>${i}</td><td>${esc(x.pattern)}</td><td><span class="chip">${esc(actName(x.action))}</span></td>
    <td style="text-align:right"><button class="btn sm danger" data-remfilter="${esc(x.pattern)}">${t("common_remove")}</button></td></tr>`).join("");
  return `<div class="cardhead"><h2>${t("filters_h")}</h2><p class="sub">${t("filters_sub")}</p></div>
    <div class="note">${t("filters_note")}</div>
    <div class="card"><h3>${t("filters_active_h")} · <b style="color:${on?'var(--ok)':'var(--mut)'}">${on?t("greets_on"):t("greets_off")}</b></h3>
      <div class="scroll"><table class="tbl"><thead><tr><th>#</th><th>${t("th_word")}</th><th>${t("th_action")}</th><th></th></tr></thead>
      <tbody>${f||`<tr><td colspan=4 class=mut>${t("filters_empty")}</td></tr>`}</tbody></table></div>
      <div class="rowend">
        <input id="fpat" placeholder="${t("filters_ph")}" style="flex:1;min-width:150px">
        <select id="fact" class="sel"><option value="block">${actName("block")}</option><option value="kick">${actName("kick")}</option><option value="ban">${actName("ban")}</option><option value="muzzle">${actName("muzzle")}</option><option value="announce">${actName("announce")}</option></select>
        <button class="btn primary" id="faddBtn">${t("filters_add")}</button>
        <button class="btn" id="fToggleBtn">${on?t("greets_disable"):t("greets_enable")}</button></div></div>`;
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
    <div class="cardgrid">
    <div class="card"><h3>${t("av_room_h")}</h3>
      <p class="sub" style="margin-bottom:12px">${t("av_room_desc")}</p>
      <div class="avbox"><img id="avImgServer" class="avimg" alt="">
        <div class="avside"><input type="file" id="avFileServer" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateServer">${t("av_upload")}</button></div></div></div>
    <div class="card"><h3>${t("av_def_h")}</h3>
      <p class="sub" style="margin-bottom:12px">${t("av_def_desc")}</p>
      <div class="avbox"><img id="avImgDefault" class="avimg" alt="">
        <div class="avside"><input type="file" id="avFileDefault" accept="image/*" style="margin-bottom:10px">
        <button class="btn primary" id="avUpdateDefault">${t("av_upload")}</button></div></div></div>
    </div>`;
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
      <label class="fld"><span>${t("srv_seedurl")}</span><input id="cfgSeedUrl" placeholder="https://astra.inbizio.xyz/api/v1/rooms"><small class="sub">${t("srv_seedurl_hint")}</small></label>
      <div class="rowend"><button class="btn primary" id="cfgSrvSave">${t("common_save_changes")}</button></div>
    </div>
    <div class="cardhead"><h2>${t("dir_h")}</h2><p class="sub">${t("dir_sub")}</p></div>
    <div class="card">
      <label class="check"><input type="checkbox" id="cfgDirEnabled"> ${t("dir_enabled")}</label>
      <label class="check"><input type="checkbox" id="cfgDirListed"> ${t("dir_listed")}</label>
      <p class="sub" style="margin:-4px 0 10px">${t("dir_listed_hint")}</p>
      <label class="fld"><span>${t("dir_desc")}</span><input id="cfgDirDesc" placeholder="${t("dir_desc_ph")}"></label>
      <label class="fld"><span>${t("dir_tags")}</span><input id="cfgDirTags" placeholder="${t("dir_tags_ph")}"><small class="sub">${t("dir_tags_hint")}</small></label>
      <label class="fld"><span>${t("dir_website")}</span><input id="cfgDirWeb" placeholder="https://"></label>
      <label class="fld"><span>${t("dir_host")}</span><input id="cfgDirHost" placeholder="chat.midominio.com"><small class="sub">${t("dir_host_hint")}</small></label>
      <label class="check"><input type="checkbox" id="cfgDirTls"> ${t("dir_tls")}</label>
      <div id="cfgDirUrl" class="sub" style="margin:10px 0"></div>
      <p class="sub">${t("dir_privacy")}</p>
      <div class="rowend"><button class="btn primary" id="cfgDirSave">${t("common_save_changes")}</button></div>
    </div>`;
}
async function fillServerCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  g("cfgRoomName").value=c.room_name||""; g("cfgRoomTopic").value=c.room_topic||"";
  g("cfgBotName").value=c.bot_name||""; g("cfgPort").value=c.port||0; g("cfgWebPort").value=c.web_port||0;
  g("cfgOwnerPw").value=c.owner_password||""; g("cfgLanguage").value=c.language||0; g("cfgDataDir").value=c.data_dir||"";
  g("cfgWebEnabled").checked=!!c.web_enabled; g("cfgAllowReg").checked=!!c.allow_registration; g("cfgRoomsearch").checked=!!c.roomsearch;
  g("cfgSeedUrl").value=c.seed_url||"";
  const d=c.directory||{};
  g("cfgDirEnabled").checked=!!d.enabled; g("cfgDirListed").checked=d.listed!==false;
  g("cfgDirDesc").value=d.description||""; g("cfgDirTags").value=(d.tags||[]).join(", ");
  g("cfgDirWeb").value=d.website||""; g("cfgDirHost").value=d.public_host||"";
  g("cfgDirTls").checked=!!d.tls;
  const url=(STATE.server||{}).directory;
  g("cfgDirUrl").innerHTML = url
    ? `${t("dir_url")} <a href="${esc(url)}" target="_blank" rel="noopener">${esc(url)}</a>`
    : (d.enabled ? t("dir_pending") : "");
}
async function saveDirectoryCfg(){
  const c = await loadConfig(); const g=(id)=>document.getElementById(id);
  c.directory = c.directory || {};
  c.directory.enabled=g("cfgDirEnabled").checked;
  c.directory.listed=g("cfgDirListed").checked;
  c.directory.description=g("cfgDirDesc").value.trim();
  // Las etiquetas se mandan como lista; el directorio descarta las que no
  // estén en su vocabulario, así que aquí no hace falta validarlas.
  c.directory.tags=g("cfgDirTags").value.split(",").map(x=>x.trim().toLowerCase()).filter(Boolean);
  c.directory.website=g("cfgDirWeb").value.trim();
  c.directory.public_host=g("cfgDirHost").value.trim();
  c.directory.tls=g("cfgDirTls").checked;
  await postConfig(c);
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
    <div class="card"><h3>${t("sec_spam_h")}</h3>
      <p class="sub" style="margin-bottom:10px">${t("sec_spam_note")}</p>
      <label class="check"><input type="checkbox" id="secSpamEnabled"> ${t("sec_spam_on")}</label>
      <label class="check"><input type="checkbox" id="secSpamRate"> ${t("sec_spam_rate")}</label>
      <label class="check"><input type="checkbox" id="secSpamDup"> ${t("sec_spam_dup")}</label>
      <div class="grid2">
        ${fld("secSpamMaxMsgs",t("sec_spam_maxmsgs"))}
        ${fld("secSpamMaxPm",t("sec_spam_maxpm"))}
        ${fld("secSpamWindow",t("sec_spam_window"))}
        ${fld("secSpamDupCount",t("sec_spam_dupcount"))}
        ${fld("secSpamMinChars",t("sec_spam_minchars"))}
        ${fld("secSpamSim",t("sec_spam_sim"))}
        ${fld("secSpamMute",t("sec_spam_mutesec"))}
        ${fld("secSpamBan",t("sec_spam_bansesec"))}
      </div>
      <label class="fld"><span>${t("sec_spam_action")}</span>
        <select class="sel" id="secSpamAction">
          <option value="warn">${t("sec_spam_act_warn")}</option>
          <option value="mute">${t("sec_spam_act_mute")}</option>
          <option value="kick">${t("sec_spam_act_kick")}</option>
          <option value="ban">${t("sec_spam_act_ban")}</option>
        </select></label></div>
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
  const sp=s.anti_spam||{};
  g("secSpamEnabled").checked=sp.enabled!==false; g("secSpamRate").checked=sp.rate_enabled!==false; g("secSpamDup").checked=sp.duplicates_enabled!==false;
  g("secSpamMaxMsgs").value=sp.max_messages??4; g("secSpamMaxPm").value=sp.max_pm??6; g("secSpamWindow").value=sp.window_secs??3;
  g("secSpamDupCount").value=sp.duplicate_count??4; g("secSpamMinChars").value=sp.min_chars??4; g("secSpamSim").value=sp.similarity_percent??90;
  g("secSpamAction").value=sp.action||"mute"; g("secSpamMute").value=sp.mute_secs??600; g("secSpamBan").value=sp.ban_secs??900;
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
  s.anti_spam=s.anti_spam||{};
  s.anti_spam.enabled=document.getElementById("secSpamEnabled").checked;
  s.anti_spam.rate_enabled=document.getElementById("secSpamRate").checked;
  s.anti_spam.duplicates_enabled=document.getElementById("secSpamDup").checked;
  s.anti_spam.max_messages=g("secSpamMaxMsgs"); s.anti_spam.max_pm=g("secSpamMaxPm"); s.anti_spam.window_secs=g("secSpamWindow");
  s.anti_spam.duplicate_count=g("secSpamDupCount"); s.anti_spam.min_chars=g("secSpamMinChars"); s.anti_spam.similarity_percent=g("secSpamSim");
  s.anti_spam.action=document.getElementById("secSpamAction").value;
  s.anti_spam.mute_secs=g("secSpamMute"); s.anti_spam.ban_secs=g("secSpamBan");
  await postConfig(c);
}

function renderProxies(){
  const rows=(STATE.trustedProxies||[]).map(ip=>`<span class="pill">${esc(ip)} <a href="#" data-rmproxy="${esc(ip)}">×</a></span>`).join("");
  return `<div class="cardhead"><h2>${t("proxy_h")}</h2><p class="sub">${t("proxy_sub")}</p></div>
    <div class="note">${t("proxy_note")}</div>
    <div class="card"><div>${rows||`<span class=mut>${t("common_none_f")}</span>`}</div>
    <div class="inline" style="margin-top:12px"><input id="proxyIn" placeholder="1.2.3.4"><button class="btn primary" id="proxyAdd">${t("common_add")}</button></div></div>`;
}

function renderVpn(){
  const v = STATE.vpn || {enabled:false,action:"report",feedUrl:"",refreshHours:24,entries:[]};
  const g = STATE.geoip || {hasAsn:false,hasCity:false,enabled:false,asnUrl:"",cityUrl:"",refreshHours:24};
  const actOpts = ["report","reject","captcha","quarantine"]
    .map(a=>`<option value="${a}"${v.action===a?" selected":""}>${t("vpn_act_"+a)}</option>`).join("");
  const allowRows=(v.allow||[]).map(a=>`<span class="pill">${esc(a)} <a href="#" data-vpnallowdel="${esc(a)}">×</a></span>`).join("");
  const detRows=(v.detections||[]).map(d=>`<tr>
    <td><code>${esc(d.ip)}</code></td><td>${esc(d.name)}</td>
    <td><span class="chip">${esc(d.rule)}</span></td><td>${esc(d.action)}</td>
    <td>${d.hits||1}</td>
    <td style="text-align:right"><button class="btn sm" data-vpnallowip="${esc(d.ip)}">${t("vpn_allow_btn")}</button></td></tr>`).join("");
  const asnBadge = g.hasAsn ? `<span class="badge voice">${t("geoip_asn_loaded")}</span>` : `<span class="badge">${t("geoip_asn_missing")}</span>`;
  const vpnCount = v.count||0;
  const feedBadge = v.refreshing
    ? `<span class="badge">${t("vpn_refreshing")}</span>`
    : (vpnCount>0 ? `<span class="badge voice">${t("vpn_count", vpnCount)}</span>`
                  : `<span class="badge">${t("vpn_empty")}</span>`);
  return `<div class="cardhead"><h2>${t("vpn_h")}</h2><p class="sub">${t("vpn_sub")}</p></div>
    <div class="note">${t("vpn_note")}</div>
    <div class="card"><h3>${t("geoip_h")} ${asnBadge}</h3>
      <p class="sub">${t("geoip_sub")}</p>
      <label class="check"><input type="checkbox" id="geoipEnabled"${g.enabled?" checked":""}> ${t("geoip_enabled")}</label>
      <label class="fld"><span>${t("geoip_asn_url")}</span><input id="geoipAsnUrl" value="${esc(g.asnUrl)}" placeholder="https://…"></label>
      <label class="fld"><span>${t("geoip_city_url")}</span><input id="geoipCityUrl" value="${esc(g.cityUrl)}" placeholder="${t("geoip_city_ph")}"></label>
      <label class="fld"><span>${t("geoip_refresh")}</span><input id="geoipHours" type="number" min="1" value="${g.refreshHours||24}"></label>
      <div class="rowend"><button class="btn" id="geoipRefreshNow">${t("geoip_refresh_now")}</button>
      <button class="btn primary" id="geoipSave">${t("common_save_changes")}</button></div>
    </div>
    <div class="card"><h3>${t("vpn_cfg_h")} ${feedBadge}</h3>
      <p class="sub">${t("vpn_count_hint")}</p>
      <label class="check"><input type="checkbox" id="vpnEnabled"${v.enabled?" checked":""}> ${t("vpn_enabled")}</label>
      <div class="grid2">
        <label class="fld"><span>${t("vpn_action")}</span><select id="vpnAction">${actOpts}</select></label>
        <label class="fld"><span>${t("vpn_refresh")}</span><input id="vpnHours" type="number" min="1" value="${v.refreshHours||24}"></label>
      </div>
      <label class="fld"><span>${t("vpn_feed")}</span><input id="vpnFeedUrl" value="${esc(v.feedUrl)}" placeholder="https://…"></label>
      <div class="rowend"><button class="btn" id="vpnRefreshNow">${t("vpn_refresh_now")}</button>
      <button class="btn" id="vpnEnforce">${t("vpn_enforce_now")}</button>
      <button class="btn primary" id="vpnSave">${t("common_save_changes")}</button></div>
    </div>
    <div class="card"><h3>${t("vpn_entries_h")}</h3>
      <div class="inline"><select id="vpnKind" class="sel sm"><option value="cidr">CIDR</option><option value="asn">ASN</option></select>
      <input id="vpnValue" placeholder="1.2.3.0/24 o 64500"><button class="btn primary" id="vpnAdd">${t("common_add")}</button></div>
      <div class="inline" style="margin-top:10px">
        <button class="btn" id="vpnClearFeed">${t("vpn_clear_feed")}</button>
        <button class="btn danger" id="vpnClearManual">${t("vpn_clear_manual")}</button>
      </div>
      <div class="inline" style="margin-top:10px">
        <input id="vpnSearch" placeholder="${t("vpn_search_ph")}" style="flex:1">
        <select id="vpnFilterKind" class="sel sm"><option value="">${t("vpn_filter_all")}</option><option value="cidr">CIDR</option><option value="asn">ASN</option></select>
        <select id="vpnPer" class="sel sm" title="${t("vpn_per_page")}">
          <option value="10"${VPNPAGE.per===10?" selected":""}>10</option>
          <option value="15"${VPNPAGE.per===15?" selected":""}>15</option>
          <option value="20"${VPNPAGE.per===20?" selected":""}>20</option>
          <option value="25"${VPNPAGE.per===25?" selected":""}>25</option>
        </select>
      </div>
      <div class="scroll"><table class="tbl"><thead><tr><th>${t("vpn_th_kind")}</th><th>${t("vpn_th_value")}</th><th>${t("vpn_th_source")}</th><th></th></tr></thead>
      <tbody id="vpnEntriesBody"><tr><td colspan=4 class=mut>${t("common_loading")}</td></tr></tbody></table></div>
      <div class="rowend">
        <button class="btn sm" id="vpnPrev">‹ ${t("vpn_prev")}</button>
        <span id="vpnPageInfo" class="mut" style="margin:0 8px"></span>
        <button class="btn sm" id="vpnNext">${t("vpn_next")} ›</button>
      </div>
    </div>
    <div class="card"><h3>${t("vpn_import_h")}</h3>
      <p class="sub">${t("vpn_import_sub")}</p>
      <textarea id="vpnImport" spellcheck="false" style="width:100%;height:18vh;font-family:ui-monospace,monospace;font-size:12.5px" placeholder="1.2.3.0/24&#10;AS64500"></textarea>
      <div class="rowend"><button class="btn primary" id="vpnImportBtn">${t("vpn_import_btn")}</button></div>
    </div>
    <div class="card"><h3>${t("vpn_allow_h")}</h3>
      <p class="sub">${t("vpn_allow_sub")}</p>
      <div>${allowRows||`<span class=mut>${t("common_none_f")}</span>`}</div>
      <div class="inline" style="margin-top:12px"><input id="vpnAllowIn" placeholder="1.2.3.4 o 1.2.3.0/24"><button class="btn primary" id="vpnAllowAdd">${t("common_add")}</button></div>
    </div>
    <div class="card"><h3>${t("vpn_det_h")} <span class="badge">${(v.detections||[]).length}</span></h3>
      <p class="sub">${t("vpn_det_sub")}</p>
      <div class="scroll"><table class="tbl"><thead><tr><th>${t("vpn_det_th_ip")}</th><th>${t("vpn_det_th_name")}</th><th>${t("vpn_det_th_rule")}</th><th>${t("vpn_det_th_action")}</th><th>${t("vpn_det_th_hits")}</th><th></th></tr></thead>
      <tbody>${detRows||'<tr><td colspan=6 class=mut>—</td></tr>'}</tbody></table></div>
      <div class="rowend"><button class="btn danger" id="vpnDetClear">${t("vpn_det_clear")}</button></div>
    </div>`;
}
/* ---------------- Entries VPN: lista paginada con búsqueda ---------------- */
const VPNPAGE = { page: 0, per: 25, q: "", kind: "", total: 0 };
function vpnEntriesRender(entries){
  const body = document.getElementById("vpnEntriesBody");
  if(!body) return;
  body.innerHTML = entries.length
    ? entries.map(e=>`<tr><td><span class="chip">${esc(e.kind)}</span></td>
        <td><code>${esc(e.value)}</code></td><td>${esc(e.source)}</td>
        <td style="text-align:right"><button class="btn sm danger" data-vpndel="${esc(e.kind)}|${esc(e.value)}">×</button></td></tr>`).join("")
    : `<tr><td colspan=4 class=mut>${t("common_none_f")}</td></tr>`;
  // Rewire los botones de borrado.
  body.querySelectorAll("[data-vpndel]").forEach(a=>a.onclick=async e=>{
    e.preventDefault();
    const [kind,value]=a.dataset.vpndel.split("|");
    await removeVpnBlock(kind,value);
    await loadVpnEntries();
  });
}
let vpnSearchTimer = null;
async function loadVpnEntries(){
  const info = document.getElementById("vpnPageInfo");
  const qs = `q=${encodeURIComponent(VPNPAGE.q)}&kind=${encodeURIComponent(VPNPAGE.kind)}&page=${VPNPAGE.page}&per=${VPNPAGE.per}`;
  const r = await api("/admin/vpn/entries?"+qs);
  if(!r.ok){ if(info) info.textContent = t("common_error"); return; }
  const j = await r.json().catch(()=>({entries:[],total:0,page:0,per:VPNPAGE.per}));
  VPNPAGE.total = j.total||0;
  VPNPAGE.page = j.page||0;
  vpnEntriesRender(j.entries||[]);
  const pages = Math.max(1, Math.ceil(VPNPAGE.total/VPNPAGE.per));
  if(info) info.textContent = t("vpn_page_info", VPNPAGE.page+1, pages, VPNPAGE.total);
  const prev=document.getElementById("vpnPrev"), next=document.getElementById("vpnNext");
  if(prev) prev.disabled = VPNPAGE.page<=0;
  if(next) next.disabled = VPNPAGE.page+1>=pages;
}
async function saveGeoipCfg(){
  const body={enabled:document.getElementById("geoipEnabled").checked,
    asnUrl:document.getElementById("geoipAsnUrl").value,
    cityUrl:document.getElementById("geoipCityUrl").value,
    refreshHours:parseInt(document.getElementById("geoipHours").value||"24",10)};
  const r=await api("/admin/geoip/config",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(body)});
  if(r.ok) toast(t("geoip_saved"),"ok"); else toast(t("err_prefix")+t("err_save"),"err");
}
async function geoipRefreshNow(){
  const r=await api("/admin/geoip/refresh",{method:"POST",headers:{"Content-Type":"application/json"},body:"{}"});
  if(r.ok) toast(t("geoip_refresh_queued"),"ok"); else toast(t("err_prefix")+t("err_save"),"err");
}

async function vpnRefreshNow(){
  const r=await api("/admin/vpn/refresh",{method:"POST",headers:{"Content-Type":"application/json"},body:"{}"});
  if(r.ok) toast(t("vpn_refresh_queued"),"ok"); else toast(t("err_prefix")+t("err_save"),"err");
}
async function vpnEnforceNow(){
  const r=await api("/admin/vpn/enforce",{method:"POST",headers:{"Content-Type":"application/json"},body:"{}"});
  const j=await r.json().catch(()=>({kicked:0,quarantined:0}));
  toast(t("vpn_enforced", j.kicked||0, j.quarantined||0),"ok");
  await refresh();
}
async function vpnAllowAdd(value){
  const v = value!=null ? value : (document.getElementById("vpnAllowIn")||{}).value;
  if(!v) return;
  const r=await api("/admin/vpn/allow",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({value:v})});
  const j=await r.json().catch(()=>({ok:false,status:"invalid"}));
  if(j.status==="exists"){ toast(t("vpn_allow_exists"),"ok"); await refresh(); }
  else if(j.ok){ toast(t("vpn_allow_added"),"ok"); await refresh(); }
  else toast(t("vpn_invalid"),"err");
}
async function vpnAllowRemove(value){
  await api("/admin/vpn/allow/remove",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({value})});
  await refresh();
}
async function vpnClearDetections(){
  await api("/admin/vpn/detections/clear",{method:"POST",headers:{"Content-Type":"application/json"},body:"{}"});
  toast(t("vpn_det_cleared"),"ok"); await refresh();
}

async function saveVpnCfg(){
  const body={enabled:document.getElementById("vpnEnabled").checked,
    action:document.getElementById("vpnAction").value,
    feedUrl:document.getElementById("vpnFeedUrl").value,
    refreshHours:parseInt(document.getElementById("vpnHours").value||"24",10)};
  const r=await api("/admin/vpn/config",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(body)});
  if(r.ok){
    const j=await r.json().catch(()=>({}));
    if((j.kicked||0)+(j.quarantined||0)>0) toast(t("vpn_enforced", j.kicked||0, j.quarantined||0),"ok");
    else toast(t("vpn_saved"),"ok");
    await refresh();
  } else toast(t("err_prefix")+t("err_save"),"err");
}
async function addVpnBlock(){
  const kind=document.getElementById("vpnKind").value, value=document.getElementById("vpnValue").value;
  const r=await api("/admin/vpn/add",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({kind,value})});
  const j=await r.json().catch(()=>({ok:false}));
  if(j.ok){ document.getElementById("vpnValue").value=""; toast(t("vpn_added"),"ok"); await refresh(); }
  else toast(t("vpn_invalid"),"err");
}
async function removeVpnBlock(kind,value){
  await api("/admin/vpn/remove",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({kind,value})});
  await refresh();
}
async function clearVpnSource(source){
  await api("/admin/vpn/clear",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({source})});
  await refresh();
}
async function importVpnFeed(){
  const text=document.getElementById("vpnImport").value;
  const r=await api("/admin/vpn/import",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({text})});
  const j=await r.json().catch(()=>({loaded:0}));
  toast(t("vpn_imported", j.loaded||0),"ok"); await refresh();
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

/* ---------------- Bot agente (múltiples) ---------------- */
let BOTCFG = null, BOTLIST = [], BOTID = 0, BOT_AVATAR = "";
function renderBot(){
  return `<div class="cardhead"><h2>${t("bot_h")}</h2><p class="sub">${t("bot_sub")}</p></div>
    <div class="note">${t("bot_note")}</div>
    <div class="card"><h3>${t("bot_select")}</h3>
      <label class="fld"><span>${t("bot_select")}</span>
        <select id="botSelect"></select></label>
      <div class="rowend">
        <button class="btn" id="botNew">${t("bot_new")}</button>
        <button class="btn danger" id="botDel">${t("bot_del")}</button>
      </div>
    </div>
    <div class="card"><h3>${t("bot_identity")}</h3>
      <label class="fld"><span class="switch"><input type="checkbox" id="botEnabled"><span class="slider"></span></span>${t("bot_enabled")}</label>
      <label class="fld"><span>${t("bot_name_l")}</span><input id="botName" placeholder="${esc(t("bot_name_ph"))}"></label>
      <label class="fld"><span>${t("bot_avatar_l")}</span><input type="file" id="botAvatarFile" accept="image/*"></label>
      <div class="avbox"><img id="botAvatarImg" class="avimg" alt="">
        <div class="avside"><button class="btn" id="botAvatarClear">${t("bot_avatar_clear")}</button></div></div>
    </div>
    <div class="card"><h3>${t("bot_greet_h")}</h3>
      <label class="fld"><span class="switch"><input type="checkbox" id="botGreetOn"><span class="slider"></span></span>${t("bot_greet_on")}</label>
      <label class="fld"><span class="switch"><input type="checkbox" id="botGreetPm"><span class="slider"></span></span>${t("bot_greet_pm")}</label>
      <label class="fld"><span class="switch"><input type="checkbox" id="botGreetLlm"><span class="slider"></span></span>${t("bot_greet_llm")}</label>
      <label class="fld"><span>${t("bot_greet_msg")}</span><input id="botGreetMsg" placeholder="${esc(t("bot_greet_ph"))}"></label>
    </div>
    <div class="card"><h3>${t("bot_reply_h")}</h3>
      <label class="fld"><span class="switch"><input type="checkbox" id="botReplyRoom"><span class="slider"></span></span>${t("bot_reply_room")}</label>
      <label class="fld"><span class="switch"><input type="checkbox" id="botReplyPm"><span class="slider"></span></span>${t("bot_reply_pm")}</label>
      <label class="fld"><span>${t("bot_trigger")}</span>
        <select id="botTrigger">
          <option value="contains">${t("bot_trigger_contains")}</option>
          <option value="prefix">${t("bot_trigger_prefix")}</option>
          <option value="always">${t("bot_trigger_always")}</option>
        </select></label>
      <label class="fld"><span>${t("bot_prefix_l")}</span><input id="botPrefix" placeholder="!"></label>
      <label class="fld"><span class="switch"><input type="checkbox" id="botMemory"><span class="slider"></span></span>${t("bot_memory")}</label>
      <div class="rowend">
        <label class="fld"><span>${t("bot_memory_turns")}</span><input id="botMemoryTurns" type="number" min="1" max="50"></label>
        <label class="fld"><span>${t("bot_history_lines")}</span><input id="botHistoryLines" type="number" min="0" max="50"></label>
        <label class="fld"><span>${t("bot_cooldown")}</span><input id="botCooldown" type="number" min="0" max="120"></label>
        <label class="fld"><span>${t("bot_max_inflight")}</span><input id="botMaxInflight" type="number" min="1" max="16"></label>
      </div>
    </div>
    <div class="card"><h3>⚡ ${t("bot_exec_h")}</h3>
      <div class="note">${t("bot_exec_note")}</div>
      <label class="fld"><span class="switch"><input type="checkbox" id="botExecCmds"><span class="slider"></span></span>${t("bot_exec_on")}</label>
      <label class="fld"><span>${t("bot_allowed_cmds")}</span><input id="botAllowedCmds" placeholder="topic, kick, ban"></label>
    </div>
    <div class="card"><h3>${t("bot_llm_h")}</h3>
      <label class="fld"><span>${t("bot_provider")}</span>
        <select id="botProvider">
          <option value="openai">${t("bot_provider_openai")}</option>
          <option value="deepseek">${t("bot_provider_deepseek")}</option>
          <option value="anthropic">${t("bot_provider_anthropic")}</option>
        </select></label>
      <label class="fld"><span>${t("bot_api_key")}</span><input id="botApiKey" type="password" required autocomplete="off"></label>
      <div class="rowend">
        <label class="fld"><span>${t("bot_model")}</span><input id="botModel" placeholder="gpt-4o-mini"></label>
        <label class="fld"><span>${t("bot_temp")}</span><input id="botTemp" type="number" step="0.1" min="0" max="2"></label>
        <label class="fld"><span>${t("bot_max_tokens")}</span><input id="botMaxTokens" type="number" min="1" max="4096"></label>
      </div>
      <label class="fld"><span>${t("bot_prompt")}</span><textarea id="botPrompt" spellcheck="false" style="height:90px" placeholder="${esc(t("bot_prompt_ph"))}"></textarea></label>
      <label class="fld"><span>${t("bot_fallback")}</span><input id="botFallback"></label>
      <details class="bothelp"><summary>❓ ${t("bot_help")}</summary>
        <p class="help-txt">${t("bot_help_intro")}</p>
        <div class="tblwrap"><table class="helptbl">
          <thead><tr>
            <th>${t("bot_help_provider")}</th>
            <th>${t("bot_help_model")}</th><th>${t("bot_help_key")}</th><th>${t("bot_help_balance")}</th>
          </tr></thead>
          <tbody>
            <tr><td>OpenAI</td><td><code>gpt-4o-mini</code></td>
              <td><a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener">platform.openai.com</a></td>
              <td>${t("bot_help_balance_openai")}</td></tr>
            <tr><td>DeepSeek</td><td><code>deepseek-v4-flash</code></td>
              <td><a href="https://platform.deepseek.com/api_keys" target="_blank" rel="noopener">platform.deepseek.com</a></td>
              <td>${t("bot_help_balance_deepseek")}</td></tr>
            <tr><td>Anthropic</td><td><code>claude-haiku-4-5</code></td>
              <td><a href="https://platform.anthropic.com/settings/keys" target="_blank" rel="noopener">platform.anthropic.com</a></td>
              <td>${t("bot_help_balance_anthropic")}</td></tr>
          </tbody>
        </table></div>
        <div class="note">${t("bot_help_note")}</div>
      </details>
    </div>
    <div class="rowend"><button class="btn primary" id="botSave">${t("common_save")}</button></div>`;
}
async function loadBots(){
  const r=await api("/admin/bots"); if(!r.ok) return;
  BOTLIST=await r.json().catch(()=>[]);
  const sel=document.getElementById("botSelect"); if(!sel) return;
  sel.innerHTML=(BOTLIST.length?BOTLIST.map(b=>`<option value="${b.id}">${esc(b.name||("id "+b.id))}${b.enabled?" ✓":""}</option>`).join(""):`<option value="0">${t("bot_none")}</option>`);
  if(BOTLIST.length && !BOTLIST.some(b=>b.id===BOTID)) BOTID=BOTLIST[0].id;
  sel.value=String(BOTID||0);
  await loadBot();
}
async function loadBot(){
  const b=BOTLIST.find(x=>x.id===BOTID);
  const c=(b&&b.config)||{};
  BOTCFG=c;
  const set=(id,v)=>{ const el=document.getElementById(id); if(el && v!=null) el.value=v; };
  const chk=(id,v)=>{ const el=document.getElementById(id); if(el) el.checked=!!v; };
  chk("botEnabled",c.enabled); set("botName",c.name);
  chk("botGreetOn",c.greet_on_join); chk("botGreetPm",c.greet_as_pm); chk("botGreetLlm",c.greet_llm); set("botGreetMsg",c.greet_message);
  chk("botReplyRoom",c.reply_in_room); chk("botReplyPm",c.reply_by_pm);
  set("botTrigger",c.trigger); set("botPrefix",c.trigger_prefix);
  chk("botMemory",c.conversation_memory); set("botMemoryTurns",c.memory_turns);
  set("botHistoryLines",c.recent_history_lines);
  set("botCooldown",c.cooldown_secs); set("botMaxInflight",c.max_in_flight);
  chk("botExecCmds",c.execute_commands); set("botAllowedCmds",(c.allowed_commands||[]).join(","));
  const llm=c.llm||{};
  set("botProvider",llm.provider); set("botApiKey",llm.api_key);
  set("botModel",llm.model); set("botTemp",llm.temperature); set("botMaxTokens",llm.max_tokens);
  set("botPrompt",llm.system_prompt); set("botFallback",c.fallback_response);
  BOT_AVATAR=c.avatar||"";
  const aimg=document.getElementById("botAvatarImg");
  if(aimg){ if(BOT_AVATAR) aimg.src="data:image/jpeg;base64,"+BOT_AVATAR; else aimg.removeAttribute("src"); }
  applyBotDefaults();
}
// Modelos por defecto por proveedor. Al cambiar el proveedor, si el modelo
// está vacío (o es default de otro proveedor) se rellena.
const BOT_API = {
  openai:{model:"gpt-4o-mini"},
  deepseek:{model:"deepseek-v4-flash"},
  anthropic:{model:"claude-haiku-4-5"},
};
function applyBotDefaults(force=false){
  const g=(id)=>document.getElementById(id);
  const p=g("botProvider").value;
  const def=BOT_API[p]; if(!def) return;
  if(force){ g("botModel").value=def.model; return; }
  const m=g("botModel").value.trim();
  const modelIsDefault=Object.entries(BOT_API).some(([name, value])=>name!==p && m===value.model);
  if(!m || modelIsDefault) g("botModel").value=def.model;
}
async function saveBot(){
  const g=(id)=>document.getElementById(id);
  if(!BOTID){ toast(t("bot_none"),"err"); return; }
  if(g("botEnabled").checked && !g("botApiKey").value.trim()){ toast(t("bot_api_key_req"),"err"); return; }
  const base=BOTCFG||{};
  const c={
    enabled:g("botEnabled").checked,
    name:g("botName").value.trim()||base.name||"",
    avatar:BOT_AVATAR,
    greet_on_join:g("botGreetOn").checked,
    greet_as_pm:g("botGreetPm").checked,
    greet_llm:g("botGreetLlm").checked,
    greet_message:g("botGreetMsg").value,
    reply_in_room:g("botReplyRoom").checked,
    reply_by_pm:g("botReplyPm").checked,
    trigger:g("botTrigger").value,
    trigger_prefix:g("botPrefix").value,
    conversation_memory:g("botMemory").checked,
    memory_turns:parseInt(g("botMemoryTurns").value)||12,
    recent_history_lines:parseInt(g("botHistoryLines").value)||0,
    cooldown_secs:parseInt(g("botCooldown").value)||3,
    max_in_flight:parseInt(g("botMaxInflight").value)||4,
    execute_commands:g("botExecCmds").checked,
    allowed_commands:g("botAllowedCmds").value.split(",").map(s=>s.trim()).filter(Boolean),
    llm:{
      provider:g("botProvider").value,
      api_key:g("botApiKey").value,
      model:g("botModel").value.trim()||base.llm?.model||"",
      temperature:parseFloat(g("botTemp").value)||0.7,
      max_tokens:parseInt(g("botMaxTokens").value)||200,
      system_prompt:g("botPrompt").value,
      timeout_secs:(base.llm&&base.llm.timeout_secs)||20,
    },
    fallback_response:g("botFallback").value,
  };
  const r=await api("/admin/bots/update",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({id:BOTID,config:c})});
  if(r.ok){ const j=await r.json().catch(()=>null); if(j&&j.id) BOTID=j.id; toast(t("bot_saved"),"ok"); await loadBots(); }
  else{ const j=await r.json().catch(()=>({})); toast(t("err_prefix")+(j.error||t("err_save")),"err"); }
}
async function newBot(){
  let name="Nova"; let n=2;
  const taken=new Set(BOTLIST.map(b=>(b.name||"").toLowerCase()));
  while(taken.has(name.toLowerCase())){ name="Nova"+n; n++; }
  const c={
    enabled:false, name,
    avatar:"",
    greet_on_join:true, greet_as_pm:true, greet_llm:true,
    greet_message:"¡Hola +n! Bienvenido a +rn. 🙂",
    reply_in_room:true, reply_by_pm:true,
    trigger:"contains", trigger_prefix:"!",
    conversation_memory:true, memory_turns:12, recent_history_lines:15,
    cooldown_secs:3, max_in_flight:4, execute_commands:false, allowed_commands:[],
    llm:{provider:"openai", api_key:"", model:"gpt-4o-mini", temperature:0.7, max_tokens:400, system_prompt:"", timeout_secs:30},
    fallback_response:"Hmm, ahora mismo no puedo responder. Inténtalo en un momento.",
  };
  const r=await api("/admin/bots",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(c)});
  if(r.ok){ const j=await r.json().catch(()=>null); if(j&&j.id) BOTID=j.id; toast(t("bot_saved"),"ok"); await loadBots(); }
  else{ const j=await r.json().catch(()=>({})); toast(t("err_prefix")+(j.error||t("err_save")),"err"); }
}
async function delBot(){
  if(!BOTID){ toast(t("bot_none"),"err"); return; }
  if(!confirm(t("bot_del_confirm"))) return;
  const r=await api("/admin/bots/delete",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({id:BOTID})});
  if(r.ok){ toast(t("bot_deleted"),"ok"); BOTID=0; await loadBots(); }
  else{ const j=await r.json().catch(()=>({})); toast(t("err_prefix")+(j.error||t("err_save")),"err"); }
}

/* ---------------- Scripts ---------------- */
let SCRIPTS={dir:"",scripts:[]};
let SCRIPT_SEARCH=[];
function scStateBadge(s){
  const map={
    active:["ok","sc_state_active"], loaded:["","sc_state_loaded"],
    error:["err","sc_state_error"], on_disk:["","sc_state_disk"], unloaded:["warn","sc_state_unloaded"]
  };
  const m=map[s]||["",s];
  return `<span class="chip ${m[0]}">${t(m[1])}</span>`;
}
function renderScripts(){
  return `<div class="cardhead"><h2>${t("sc_h")}</h2><p class="sub">${t("sc_sub")}</p></div>
    <div class="note">${t("sc_note")}</div>
    <div class="card">
      <div class="cardhead" style="display:flex;align-items:center;justify-content:space-between;gap:8px;margin:0 0 12px">
        <h3 style="margin:0">${t("sc_installed_h")}</h3>
        <button class="btn sm" id="scRefresh">${t("sc_refresh")}</button>
      </div>
      <div class="ucards" id="scList">${t("common_loading")}</div>
    </div>
    <div class="card"><h3>${t("sc_community_h")}</h3>
      <div class="note">${t("sc_community_note")}</div>
      <div class="inline"><input id="scSearch" placeholder="${esc(t("sc_search_ph"))}"><button class="btn primary" id="scSearchBtn">${t("sc_search_btn")}</button></div>
      <div class="ucards" id="scResults" style="margin-top:12px"></div>
    </div>`;
}
async function loadScripts(){
  const box=document.getElementById("scList"); if(!box) return;
  const r=await api("/admin/scripts");
  if(!r.ok){ box.innerHTML=`<div class="empty">${t("common_error")}</div>`; return; }
  SCRIPTS=await r.json().catch(()=>({dir:"",scripts:[]}));
  renderScriptsList();
}
function renderScriptsList(){
  const box=document.getElementById("scList"); if(!box) return;
  const list=SCRIPTS.scripts||[];
  if(!list.length){ box.innerHTML=`<div class="empty">${t("sc_empty")}</div>`; return; }
  box.innerHTML=list.map(s=>{
    const kind=s.folder?t("sc_folder"):t("sc_file");
    const meta=[kind];
    if(s.fileCount) meta.push(t("sc_files",s.fileCount));
    if(s.mainFile) meta.push(esc(s.mainFile));
    const err=s.error?`<div class="warnbox" style="margin:9px 0 0">${t("sc_error_label")}: ${esc(s.error)}</div>`:"";
    const actions=s.loaded
      ? `<button class="btn sm" data-scview="${esc(s.name)}">${t("sc_view")}</button>
         <button class="btn sm" data-scload="${esc(s.name)}">${t("sc_reload")}</button>
         <button class="btn sm danger" data-sckill="${esc(s.name)}">${t("sc_unload")}</button>`
      : `<button class="btn sm" data-scview="${esc(s.name)}">${t("sc_view")}</button>
         <button class="btn sm primary" data-scload="${esc(s.name)}">${t("sc_load")}</button>`;
    return `<div class="ucard"><div class="uhead"><span class="uname">${esc(s.name)}</span>${scStateBadge(s.state)}</div>
      <div class="umeta">${meta.join(" · ")}</div>
      <div class="uactions">${actions}</div>${err}</div>`;
  }).join("");
}
async function viewScript(name, file){
  const qs="name="+encodeURIComponent(name)+(file?"&file="+encodeURIComponent(file):"");
  const r=await api("/admin/scripts/source?"+qs);
  const j=await r.json().catch(()=>({error:"error"}));
  if(!r.ok){ toast(t("err_prefix")+(j.error||t("common_error")),"err"); return; }
  showOutput(t("sc_source_title",j.name||name)+(j.file?" — "+j.file:""), [j.source||""]);
}
async function setScriptLoaded(name, load){
  const ep=load?"/admin/scripts/load":"/admin/scripts/kill";
  const r=await api(ep,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({name})});
  const j=await r.json().catch(()=>({error:"error"}));
  if(r.ok){ toast(load?t("sc_load_ok"):t("sc_unload_ok"),"ok"); await loadScripts(); }
  else{ toast(t("err_prefix")+(j.error||t("common_error")),"err"); }
}
async function searchCommunity(){
  const box=document.getElementById("scResults"); if(!box) return;
  const q=(document.getElementById("scSearch")||{}).value||"";
  box.innerHTML=`<div class="empty">${t("sc_searching")}</div>`;
  const r=await api("/admin/scripts/search?q="+encodeURIComponent(q.trim()));
  const j=await r.json().catch(()=>({error:"error"}));
  if(!r.ok){ box.innerHTML=`<div class="warnbox">${t("sc_search_err")}: ${esc(j.error||"")}</div>`; return; }
  SCRIPT_SEARCH=Array.isArray(j)?j:[];
  if(!SCRIPT_SEARCH.length){ box.innerHTML=`<div class="empty">${t("sc_no_results")}</div>`; return; }
  box.innerHTML=SCRIPT_SEARCH.map(s=>`<div class="ucard">
    <div class="uhead"><span class="uname">${esc(s.full_name)}</span>${s.stars?`<span class="chip">${t("sc_stars",s.stars)}</span>`:""}</div>
    <div class="umeta">${esc(s.description||"")}</div>
    <div class="uactions"><button class="btn sm primary" data-scinstall="${esc(s.full_name)}">${t("sc_install")}</button></div>
  </div>`).join("");
}
async function installCommunity(path){
  const b=document.querySelector(`[data-scinstall="${path}"]`);
  if(b){ b.disabled=true; b.textContent=t("sc_installing"); }
  const r=await api("/admin/scripts/install",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({path})});
  const j=await r.json().catch(()=>({error:"error"}));
  if(b){ b.disabled=false; b.textContent=t("sc_install"); }
  if(r.ok){ toast(t("sc_installed_ok"),"ok"); await loadScripts(); }
  else{ toast(t("err_prefix")+(j.error||t("sc_install_err")),"err"); }
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

/* ---------------- Soporte (reportes a GitHub) ---------------- */
function renderSoporte(){
  const gh = STATE.github||{};
  const direct = !!gh.configured;
  const hint = direct ? t("sup_direct_hint") : t("sup_no_token");
  return `<div class="cardhead"><h2>${t("sup_h")}</h2><p class="sub">${t("sup_sub")}</p></div>
    <div class="note">${t("sup_note")}</div>
    <div class="card">
      <label class="fld"><span>${t("sup_kind_label")}</span>
        <select id="supKind"><option value="bug">${t("sup_kind_bug")}</option><option value="idea">${t("sup_kind_idea")}</option></select></label>
      <label class="fld"><span>${t("sup_title_label")}</span><input id="supTitle" maxlength="256" placeholder="${t("sup_title_ph")}"></label>
      <label class="fld"><span>${t("sup_desc_label")}</span><textarea id="supDesc" rows="8" placeholder="${t("sup_desc_ph")}"></textarea></label>
      <div class="rowend">
        <button class="btn" id="supOpen">${t("sup_open")}</button>
        ${direct?`<button class="btn primary" id="supSend">${t("sup_send")}</button>`:''}
      </div>
      <p class="sub" style="margin:12px 0 0">${hint}</p>
      <p class="sub" style="margin:6px 0 0">${t("sup_open_hint")}</p>
      <p class="sub" id="supMsg" style="margin:8px 0 0"></p>
    </div>`;
}
function supBody(kind, desc){
  const type = kind==="idea" ? "Idea / Improvement" : "Bug";
  return "**Type:** "+type+"\n\n"+(desc||"");
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
  document.querySelectorAll("[data-act]").forEach(b=>b.onclick=async()=>{
    const n=b.dataset.n, a=b.dataset.act;
    if(a==="whois"||a==="id"||a==="oldname"||a==="locate"||a==="trace"){ showOutput(n, await cmd(`/${a} ${n}`)); return; }
    if(a==="customname"){
      const v=prompt(t("prompt_customname",n));
      if(v==null||!v.trim())return;
      run(`/customname ${n} ${v}`, t("toast_customname")); return;
    }
    if(a==="ban"&&!confirm(t("cf_ban",n)))return;
    if(a==="ban10"&&!confirm(t("cf_ban10",n)))return;
    if(a==="ban60"&&!confirm(t("cf_ban60",n)))return;
    if(a==="kick"&&!confirm(t("cf_kick",n)))return;
    const msg={kick:t("toast_kicked",n),ban:t("toast_banned",n),muzzle:t("toast_muted",n),unmuzzle:t("toast_unmuted",n),
      ban10:t("toast_banned",n),ban60:t("toast_banned",n),unecho:t("toast_echo_off"),uncustomname:t("toast_customname")}[a];
    run(`/${a} ${n}`, msg || t("toast_toggled"));
  });
  document.querySelectorAll("[data-autologin]").forEach(s=>s.onchange=()=>{
    const n=s.dataset.autologin, v=s.value; if(!v) return;
    run(`/addautologin ${n} ${v}`, t("toast_autologin_added",n)); s.value="";
  });
  document.querySelectorAll("[data-remautologin]").forEach(b=>b.onclick=()=>run(`/remautologin ${b.dataset.remautologin}`,t("toast_autologin_rem")));
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
  if(g("fToggleBtn"))g("fToggleBtn").onclick=()=>run(`/filter ${STATE.filtersEnabled===false?"on":"off"}`,t("toast_toggled"));
  if(g("cmdRun")){const rc=()=>{const l=g("cmdIn").value.trim(); if(l){run(l); g("cmdIn").value="";}}; g("cmdRun").onclick=rc; g("cmdIn").onkeydown=e=>{if(e.key==="Enter")rc();};}
  if(g("tomlEd")){ loadSettings(); g("tomlSave").onclick=saveSettings; g("tomlReload").onclick=loadSettings; }
  if(g("motdEd")){ loadMotd(); g("motdSave").onclick=saveMotd; }
  if(g("botSave")){ loadBots(); g("botSave").onclick=saveBot; g("botSelect").onchange=()=>{ BOTID=parseInt(g("botSelect").value)||0; loadBot(); }; g("botNew").onclick=newBot; g("botDel").onclick=delBot; g("botProvider").onchange=()=>applyBotDefaults(true); }
  if(g("scList")){
    loadScripts();
    g("scRefresh").onclick=loadScripts;
    g("scSearchBtn").onclick=searchCommunity;
    g("scSearch").onkeydown=e=>{ if(e.key==="Enter") searchCommunity(); };
    g("scList").onclick=e=>{
      const b=e.target.closest("button"); if(!b) return;
      if(b.dataset.scview!==undefined) viewScript(b.dataset.scview);
      else if(b.dataset.scload!==undefined) setScriptLoaded(b.dataset.scload,true);
      else if(b.dataset.sckill!==undefined){ if(confirm(t("sc_kill_confirm",b.dataset.sckill))) setScriptLoaded(b.dataset.sckill,false); }
    };
    g("scResults").onclick=e=>{
      const b=e.target.closest("button"); if(!b||b.dataset.scinstall===undefined) return;
      installCommunity(b.dataset.scinstall);
    };
  }
  if(g("botAvatarFile")){ g("botAvatarFile").onchange=()=>{ const f=g("botAvatarFile").files[0]; if(!f) return; const rd=new FileReader(); rd.onload=()=>{ const url=rd.result||""; BOT_AVATAR=url.split(",")[1]||""; const img=g("botAvatarImg"); if(img) img.src=url; }; rd.readAsDataURL(f); }; }
  if(g("botAvatarClear")){ g("botAvatarClear").onclick=()=>{ BOT_AVATAR=""; const img=g("botAvatarImg"); if(img) img.removeAttribute("src"); }; }
  if(g("tplEd")){ loadPlantillas(); g("tplSave").onclick=savePlantillas; }
  if(g("cfgSrvSave")){ fillServerCfg(); g("cfgSrvSave").onclick=saveServerCfg; }
  if(g("cfgDirSave")){ g("cfgDirSave").onclick=saveDirectoryCfg; }
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
  if(g("geoipSave")) g("geoipSave").onclick=saveGeoipCfg;
  if(g("geoipRefreshNow")) g("geoipRefreshNow").onclick=geoipRefreshNow;
  if(g("vpnSave")) g("vpnSave").onclick=saveVpnCfg;
  if(g("vpnRefreshNow")) g("vpnRefreshNow").onclick=vpnRefreshNow;
  if(g("vpnEnforce")) g("vpnEnforce").onclick=vpnEnforceNow;
  if(g("vpnAdd")) g("vpnAdd").onclick=addVpnBlock;
  if(g("vpnClearFeed")) g("vpnClearFeed").onclick=()=>clearVpnSource("feed");
  if(g("vpnClearManual")) g("vpnClearManual").onclick=()=>clearVpnSource("manual");
  if(g("vpnImportBtn")) g("vpnImportBtn").onclick=importVpnFeed;
  if(g("vpnAllowAdd")) g("vpnAllowAdd").onclick=()=>vpnAllowAdd();
  if(g("vpnDetClear")) g("vpnDetClear").onclick=vpnClearDetections;
  // Lista de entries paginada: se carga al renderizar la tab y se re-carga
  // en búsqueda/filtro/paginación (sin re-renderizar todo el panel).
  if(g("vpnEntriesBody")){
    loadVpnEntries();
    const s=g("vpnSearch"), f=g("vpnFilterKind"), pp=g("vpnPer");
    if(s) s.oninput=()=>{ clearTimeout(vpnSearchTimer); vpnSearchTimer=setTimeout(()=>{ VPNPAGE.q=s.value.trim(); VPNPAGE.page=0; loadVpnEntries(); }, 250); };
    if(f) f.onchange=()=>{ VPNPAGE.kind=f.value; VPNPAGE.page=0; loadVpnEntries(); };
    if(pp) pp.onchange=()=>{ VPNPAGE.per=parseInt(pp.value,10)||25; VPNPAGE.page=0; loadVpnEntries(); };
    if(g("vpnPrev")) g("vpnPrev").onclick=()=>{ if(VPNPAGE.page>0){ VPNPAGE.page--; loadVpnEntries(); } };
    if(g("vpnNext")) g("vpnNext").onclick=()=>{ VPNPAGE.page++; loadVpnEntries(); };
  }
  document.querySelectorAll("[data-vpndel]").forEach(a=>a.onclick=async e=>{
    e.preventDefault();
    const [kind,value]=a.dataset.vpndel.split("|");
    await removeVpnBlock(kind,value);
  });
  document.querySelectorAll("[data-vpnallowdel]").forEach(a=>a.onclick=async e=>{
    e.preventDefault(); await vpnAllowRemove(a.dataset.vpnallowdel);
  });
  document.querySelectorAll("[data-vpnallowip]").forEach(a=>a.onclick=async e=>{
    e.preventDefault();
    const ip=a.dataset.vpnallowip;
    // Permitir una /32 (la IP exacta detectada).
    await vpnAllowAdd(ip);
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
  if(g("supOpen")||g("supSend")){
    const compose=()=>({
      kind:g("supKind").value,
      title:g("supTitle").value.trim(),
      desc:g("supDesc").value.trim(),
    });
    if(g("supOpen")) g("supOpen").onclick=()=>{
      const {kind,title,desc}=compose();
      if(!title){ toast(t("sup_title_required"),"err"); return; }
      const repo=(STATE.github&&STATE.github.repo)||"bsjaramillo/astra";
      const url="https://github.com/"+repo+"/issues/new?title="+encodeURIComponent(title)+"&body="+encodeURIComponent(supBody(kind,desc));
      window.open(url,"_blank","noopener");
    };
    if(g("supSend")) g("supSend").onclick=async()=>{
      const {kind,title,desc}=compose();
      if(!title){ toast(t("sup_title_required"),"err"); return; }
      const msg=g("supMsg"); msg.textContent=t("sup_sending");
      const r=await api("/admin/issue",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({title,body:supBody(kind,desc)})});
      const j=await r.json().catch(()=>({error:"error"}));
      if(r.ok&&j.url){
        msg.innerHTML=t("sup_ok")+' <a href="'+esc(j.url)+'" target="_blank" rel="noopener">'+esc(j.url)+"</a>";
        toast(t("sup_ok"),"ok");
      } else {
        msg.textContent="";
        toast(t("err_prefix")+(j.error||t("sup_err")),"err");
      }
    };
  }
}

document.getElementById("menuBtn").onclick=openDrawer;
document.getElementById("backdrop").onclick=closeDrawer;
document.getElementById("refreshBtn").onclick=()=>{ CONFIG=null; refresh(); };
document.getElementById("logoutBtn").onclick=logout;
document.getElementById("modalClose").onclick=closeModal;
document.getElementById("modal").onclick=e=>{ if(e.target.id==="modal") closeModal(); };
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
