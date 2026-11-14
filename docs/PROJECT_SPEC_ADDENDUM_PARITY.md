# ADDENDUM — Paridad funcional con Burp Suite Professional

> Amplía [PROJECT_SPEC.md](PROJECT_SPEC.md) (recibido el 2026-10-22).
> Seguimiento en [BURP_PARITY_MATRIX.md](BURP_PARITY_MATRIX.md).

Objetivo técnico a largo plazo: **alcanzar paridad funcional** con las
principales capacidades de Burp Suite Professional mediante implementaciones
propias, y posteriormente superarlas en determinadas áreas.

NO copiar: código; assets; textos; iconografía; interfaz; arquitectura
interna; nombres protegidos; implementaciones propietarias. Replicar
únicamente conceptos y capacidades mediante ingeniería propia.

## 67. Burp Pro Parity Matrix
Crear `docs/BURP_PARITY_MATRIX.md` antes de considerar estable cualquier
versión importante, inventariando todas las funciones relevantes de Burp
Suite Professional. Columnas mínimas: Burp capability | ChosProxy equivalent |
Status | Milestone | Tests | Notes. Estados: ⬜ NOT IMPLEMENTED · 🔵 PLANNED ·
🟡 PARTIAL · 🟢 FEATURE COMPLETE · ⭐ CHOSPROXY SUPERSET. No declarar paridad
mientras existan capacidades importantes sin equivalente, justificación de
no aplicabilidad o milestone asignado. Actualizarla cuando cambie la
referencia funcional de Burp.

## 68. OAST / Collaborator equivalent — ChosOAST
Generar identificadores únicos; asociarlos con proyectos y requests;
registrar interacciones (timestamp, protocolo, origen, metadata) y findings
relacionados. DNS, HTTP, HTTPS y protocolos adicionales vía plugins. Modos:
Local OAST, Self-hosted OAST (preferente), External OAST provider plugin.
Mostrar qué request originó cada interacción (Finding → Original request →
OAST identifier → Interaction → Timestamp → Evidence). Sin dependencia
obligatoria de infraestructura cloud de ChosProxy.

## 69. DOM Security Analyzer — DOM Inspector
Equivalente conceptual de DOM Invader, integrado con el navegador de
laboratorio: sources; sinks; data flow; DOM XSS; postMessage; message
listeners; client-side redirects; APIs DOM peligrosas; indicadores de
prototype pollution y DOM clobbering. Visualizar SOURCE → TRANSFORMATION →
SINK, con source, sink, stack trace, archivo, línea, contexto y valor
observado. Instrumentación dinámica en el navegador aislado, no solo strings
estáticos.

## 70. Clickjacking tool — FrameLab
Analizar páginas propias/autorizadas: posibilidad de embedding;
X-Frame-Options; CSP frame-ancestors; comportamiento en iframe. Generar PoC
local solo para validación autorizada; visualizar posiciones y overlays.

## 71. Runtime instrumentation
Equivalente conceptual de Infiltrator: detectar cuándo datos de entrada
atraviesan código sensible en aplicaciones instrumentadas. Módulo separado,
nunca instalado ni inyectado automáticamente en terceros. Requiere
instrumentación explícita, entorno local/de test y autorización. Resultados
integrados en Findings.

## 72. Engagement Tools
Menú con: Find References; Target Analyzer (hosts, endpoints, parámetros,
forms, APIs, tecnologías, status, MIME, superficie de ataque estimada);
Content Discovery (scope, rate limiting, profundidad, extensiones,
diccionario configurable, pause/resume, estadísticas); CSRF PoC Generator
(explicado y revisable); Scheduled Tasks (once, periodic, at time; nunca
amplían el scope); Manual Traffic Simulator.

## 73. Advanced Content Discovery
Separado del Crawler. Wordlist configurable; extension guessing; patrones de
nombres y directorios; convenciones descubiertas; similitud de respuestas;
baselines; detección de soft-404; límites de recursión; exclusiones.
Integrado con Site Map.

## 74. Site Map advanced
Comparar site maps; snapshots; diff entre sesiones; filtros por status,
source, extensión, MIME, parámetros; ocultar duplicados; anotaciones;
colores/tags. Diff A vs B: endpoint nuevo/eliminado, parámetro nuevo,
response modificada, status modificado.

## 75. Advanced Proxy
Invisible proxying (labs configurados explícitamente); TLS passthrough
(host, puerto, regex, scope); upstream proxy con reglas por destino; SOCKS
(con resolución DNS a través de SOCKS); autenticación de red empresarial
modular; timeouts configurables (connect, read, write, DNS, retries).

## 76. Match & Replace advanced
Subsistema propio para request, response, WebSocket cliente→servidor y
servidor→cliente. Matching: literal, regex, campo HTTP estructurado, script.
Acciones: replace, add, remove, transform. Cada regla: enabled, scope,
direction, priority, description.

## 77. Scripting — ChosScripts
Scripting ligero (estilo Bambda) para filtros, columnas, transforms, Match &
Replace, acciones de Repeater, análisis y clasificación. Sin acceso al SO por
defecto; preferentemente sandbox WASM; permisos explícitos (`http.read`,
`http.modify`, `project.read`, `finding.create`).

## 78. Custom scan check DSL — ChosChecks
DSL declarativo (estilo BChecks): metadata; conditions; request/response
selectors; matchers; extraction; severity; confidence; evidence;
remediation. Checks importados inspeccionables antes de ejecutarse; enable,
disable, trust status, permisos, versión, autor.

## 79. Extension platform — ChosProxy Extension API
API estable y versionada (v1, v2…): tabs; menús; context actions; observar y
modificar mensajes; enviar requests; scanner checks; findings; site map;
project store; transforms; custom columns; servicios externos.

## 80. Extension marketplace — ChosHub
Buscar, instalar, actualizar, desinstalar, habilitar, deshabilitar; permisos;
versión; autor; firma; hash; repositorio fuente. Sin instalación silenciosa;
permisos mostrados antes de instalar.

## 81. Configuration Library
Presets versionados en JSON para crawl, audit, scope, proxy, resource pools,
autenticación y reporting (create, duplicate, import, export, edit, delete).
Ejemplos: Lightweight Crawl, Passive Audit, Balanced Audit, Local Lab, API
Audit. Sin perfiles agresivos habilitados automáticamente.

## 82. Scan Launcher
Asistente: Crawl; Audit; Crawl + Audit; Audit Selected Requests; API Scan.
Antes de ejecutar mostrar target, scope, autenticación, configuración,
resource pool, límites estimados, ajustes de seguridad y un resumen final.

## 83. Authenticated crawling
Simple login (credenciales explícitas para labs autorizados) y Recorded
Login Sequence (open URL, click, fill, submit, wait, verify; record, edit,
replay, test). Preparado para multi-step, SSO, TOTP, WebAuthn cuando el
navegador lo permita. Authentication Status Check. Nunca guardar
credenciales en texto plano sin advertencia; usar el almacén seguro del
sistema.

## 84. Browser-powered crawling
Crawler Controller → Chromium → DOM → Network → Site Map. Detectar
navegación SPA, rutas dinámicas, XHR, fetch, WebSockets, forms, enlaces
dinámicos, client-side routing. Navegador aislado.

## 85. Insertion Point Engine
Componente explícito: query, body, JSON, XML, multipart, cookies, headers,
path segments, nombres de parámetro, estructuras anidadas. Estado por punto:
Pending, Testing, Completed, Skipped, Error. Límites contra explosión
combinatoria.

## 86. Scanner phase engine
Fases mínimas: Passive Analysis, Active Analysis, JavaScript Analysis, con
concurrencia controlada. Cada audit item muestra fases, insertion points,
requests, findings, errores y duración.

## 87. Scan result debugging
Cada tarea de scanner: Summary, Issues, Audit Items, Event Log, HTTP Logger,
Audit Log, Crawler View. Saber qué hace el scanner, por qué, sobre qué
parámetro, cuántas requests lleva y qué falló.

## 88. Resource Pools
Max concurrent requests; requests/sec; delay; jitter opcional; retries;
timeout. Aplicable a crawler, scanner, fuzzer, live tasks y content
discovery. Cambiar el pool de una tarea pausada o cuando sea seguro.

## 89. Live Tasks complete
Live Passive Crawl, Live Passive Audit, Live Audit; fuentes Proxy, Repeater,
Fuzzer, Browser; tool scope, URL scope, deduplicación, configuración de
scanner, resource pool. Todo tráfico automático identificable.

## 90. Scanner deduplication
Fingerprints por método, URL normalizada, nombres de parámetros, estructura
del body y content type. Diferenciar mismo endpoint / misma estructura /
misma request exacta.

## 91. Scanner state management
Flujos Login → Navigate → Obtain Token → Request; cookies, tokens CSRF,
campos ocultos cambiantes, expiración de sesión, estado de autenticación;
reglas de extracción y actualización automática de variables.

## 92. Finding deduplication & evidence
Un issue con varias evidencias y ubicaciones afectadas; fingerprint
configurable; estados Confirmed, Needs Review, False Positive, Accepted
Risk, Fixed, Retest Needed.

## 93. False positive management
Mark false positive; suppress equivalent / URL / tipo; restore; comentarios.
Supresiones registradas.

## 94. Scan configuration
Crawl strategy; audit strategy; insertion points; checks; autenticación;
resource limits; request limits; timeouts; scope; análisis JavaScript;
presets.

## 95. API scanning complete
Importar OpenAPI, Postman Collection, SOAP WSDL; árbol API → endpoint →
métodos → parámetros; combinar definiciones con tráfico observado; detectar
definiciones durante el crawling.

## 96. GraphQL workspace
Endpoint, operaciones observadas, variables, operation names, estructuras de
respuesta; integrado con Repeater, Site Map, Search e Inspector.

## 97. Session rule engine
Reglas Before request / After response / On authentication failure / On
token expiration; acciones Extract value, Set variable, Update cookie,
Update header, Run login sequence; pipeline visual.

## 98. Project vs user settings
User settings globales y project settings del proyecto actual, con override
"Use global / Override for project".

## 99. Settings import/export
JSON; export seleccionado/proyecto/usuario; import; validate; preview
changes. Nunca aplicar configuraciones externas sin validarlas.

## 100. Automatic project backups
Intervalo, máximo de backups, solo in-scope, destino; Restore Backup; nunca
sobrescribir automáticamente el último backup válido.

## 101. Advanced network settings
Settings → Network: upstream proxies, SOCKS, DNS, TLS, timeouts, connection
reuse, versiones HTTP, autenticación, TLS passthrough.

## 102. HTTP/2 first-class support
Mantener pseudo-headers, streams, multiplexing y metadatos del protocolo;
representación comprensible sin perder información; tests HTTP/2
específicos.

## 103. Request Engine centralization
Un único motor de envío para Repeater, Scanner, Crawler, Fuzzer, Plugins e
IA: DNS, TCP, TLS, HTTP/1.1, HTTP/2, proxies, SOCKS, cookies opcionales,
connection pooling, redirects, cancelación, timing, resource pools.

## 104. REST / Automation API
Desactivada por defecto; autenticación obligatoria; bind 127.0.0.1; nunca
expuesta públicamente de forma automática. Project info, tareas, findings,
site map, start/stop de tareas permitidas, exportación.

## 105. CLI / headless foundation
`chosproxy`, `chosproxy gui`, `chosproxy project`, `chosproxy scan`. El core
nunca depende de React/Tauri.

## 106. AI assistant — ChosAI
Modos Disabled / Local Model / External Provider (local como ciudadano de
primera clase: Ollama, endpoints OpenAI-compatibles locales, LM Studio,
plugins). Explain request/response/JavaScript/finding; resumir endpoints;
comparar respuestas; sugerir investigación; generar texto de informe.
Mostrar siempre qué se enviará a un modelo externo antes de hacerlo.

## 107. Agentic testing assistant — ChosAgent
Solo herramientas deterministas de ChosProxy, sin acceso arbitrario al SO.
Permisos por herramienta DENY / ASK / ALLOW impuestos técnicamente por la
capa de herramientas (no solo por el prompt).

## 108. Agent tasks
Task, subtask, queued message, pause, stop, resume. AI Activity Log (acción,
herramienta, timestamp, target, permiso, resultado). Requests de la IA
visibles en Logger.

## 109. AI context attachments
Adjuntar request, response, endpoint, rama del site map, finding,
JavaScript, tarea de scanner.

## 110. AI privacy
Nunca enviar el proyecto completo. Preview AI Context antes de providers
externos. Redacción de Authorization, cookies, tokens, contraseñas, API keys
y regex personalizadas.

## 111. Explainer
Explicar header, cookie, parámetro, JavaScript, JWT, response, finding con
conocimiento interno, documentación, IA local o proveedor externo opcional.

## 112. Custom Repeater actions
Decode selection; Pretty JSON; Extract JWT; Compare with previous; Run
script; Analyze. Extensible con ChosScripts y plugins.

## 113. Context menu system
Menú compartido desde cualquier request: Send to Repeater / Fuzzer /
Comparer / Organizer; Analyze; Add to / Exclude from Scope; Copy; Export;
Plugin Actions.

## 114. Global filter framework — FilterEngine
Condiciones: tool source, method, host, path, status, MIME, extension, scope,
annotation, finding, regex, protocol. Usado en Proxy, Logger, Site Map,
WebSockets, Scanner, Organizer.

## 115. Find References
Global: endpoint, parámetro, dominio, archivo, string, buscando en
responses, scripts, site map y requests, mostrando la relación origen →
referencia.

## 116. Scheduler
Once / At time / Repeated interval para tareas autorizadas (crawl, report,
passive analysis, backup). Nunca automatizaciones ofensivas ocultas.

## 117. Temporary file management
Cachés, bodies grandes, perfiles de navegador, certificados temporales,
exports; directorio temporal, tamaño máximo de caché, limpieza al salir.
Nunca borrar archivos de proyecto por error.

## 118. Crash recovery
Recuperar estado, preservar el proyecto, detectar tareas interrumpidas sin
reiniciarlas automáticamente si generan tráfico. Mensaje "Previous session
ended unexpectedly." con Restore / Open read-only / Discard recovery.

## 119. Performance profiler
RAM, CPU, tamaño de BD, requests en cola, conexiones activas, tareas,
request rate. Solo local.

## 120. Security Center
Panel Security & Privacy: estado de la CA, ubicación de la clave privada,
listeners expuestos, REST API, plugins y permisos, proveedor de IA,
conexiones externas, cifrado del proyecto. Avisos (p. ej. proxy en 0.0.0.0).

## 121. Encrypted projects
Opcional; protege tráfico, tokens, findings y notas. Primitivas auditadas,
nunca criptografía propia.

## 122. Audit log
Scope changed; plugin installed; plugin permission changed; scanner
started/stopped; finding edited/suppressed; project exported; AI action
approved.

## 123. Parity test suite
`tests/parity/` con IDs (PROXY-001, REPEATER-001, SCANNER-001, OAST-001,
DOM-001…) enlazando implementación, documentación y test de cada fila de la
matriz.

## 124. Feature discovery audit
Antes de 1.0, revisar la documentación pública actual de las herramientas de
referencia y clasificar cada función: IMPLEMENTED, PARTIAL, NOT APPLICABLE,
PLANNED. Capacidades nuevas: primero especificación y matriz, nunca
implementación improvisada.

## 125. Nuevos milestones
M18 OAST/ChosOAST · M19 DOM Inspector · M20 Engagement Tools · M21 Advanced
Proxy & Network · M22 ChosChecks/ChosScripts · M23 Extension Platform/ChosHub
· M24 Authenticated Browser Crawling · M25 Advanced Scanner Engine · M26
Resource Pools & Live Tasks · M27 API/GraphQL Workspace · M28 REST
API/Headless Foundation · M29 ChosAI · M30 ChosAgent · M31
Hardening/Recovery/Encryption · M32 Burp Professional Parity Audit.

## 126. Niveles del producto
**Core:** Proxy, Intercept, History, Repeater, Site Map, WebSockets, Decoder,
Comparer, Organizer. **Pro:** Crawler, Scanner, Fuzzer, Sequencer, OAST, DOM
Inspector, Content Discovery, authenticated crawling, Live Tasks, Resource
Pools, ChosChecks, reporting avanzado, plataforma de extensiones. **Next:**
ChosAI, ChosAgent, Visual Request Flow, timelines avanzados, diff automático
de respuestas, IA local, explicaciones educativas, UX moderna, flujos entre
herramientas.

## 127. Regla de paridad
Una función solo cuenta como implementada si: realiza realmente su trabajo;
funciona con tráfico real; está integrada con el resto; persiste cuando
corresponde; maneja errores; tiene tests; está documentada; respeta el
scope; respeta los resource pools; no compromete la seguridad de ChosProxy.

## 128. Regla de diferenciación
Al alcanzar una función equivalente, preguntarse cómo hacerla mejor:
interfaz más clara, explicación educativa, menos clicks, integración,
visualización, velocidad, automatización controlada, IA local, privacidad,
extensibilidad. Objetivo: una suite propia y técnicamente seria, no "una
copia barata".

## 129. Instrucción inmediata
Leer la especificación; incorporar el addendum; crear la matriz; revisar los
milestones; asignar cada feature; identificar dependencias; identificar qué
componentes del core deben diseñarse ahora; actualizar ARCHITECTURE.md y
ROADMAP.md; NO implementar todavía las funciones avanzadas; después
continuar con el milestone actual. Las features futuras no justifican
sobrearquitecturar el código actual, pero las interfaces fundamentales deben
permitir su incorporación.
