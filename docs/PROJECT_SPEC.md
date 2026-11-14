# PROYECTO: CHOSPROXY — Especificación

> Especificación original del proyecto, tal como la definió el propietario
> (2026-10-21). Ampliada por
> [PROJECT_SPEC_ADDENDUM_PARITY.md](PROJECT_SPEC_ADDENDUM_PARITY.md).
> Cambios a esta especificación: primero aquí, después en la
> [matriz de paridad](BURP_PARITY_MATRIX.md) y en [ROADMAP.md](../ROADMAP.md).

Quiero que construyas una aplicación profesional de escritorio para auditoría de seguridad web autorizada, inspirada funcionalmente en suites como Burp Suite Professional, pero desarrollada desde cero, con código, arquitectura, interfaz, identidad visual y experiencia de usuario propias.

NO copies código, recursos gráficos, textos, iconos, diseño exacto, nombres internos ni elementos protegidos de Burp Suite/PortSwigger.

Burp debe utilizarse únicamente como referencia conceptual sobre qué problemas debe resolver una suite profesional de seguridad web.

El objetivo es crear una herramienta seria, modular, rápida, moderna y open source, especialmente diseñada para Kali Linux.

Nombre provisional: **ChosProxy**. El nombre debe poder cambiarse fácilmente desde una configuración central del proyecto.

## 1. Objetivo del producto

ChosProxy debe convertirse progresivamente en una suite completa para: inspección HTTP/HTTPS; interceptación de tráfico; modificación de requests y responses; repetición manual de peticiones; análisis de aplicaciones web; crawling; análisis de APIs; WebSockets; organización de endpoints; análisis pasivo de seguridad; auditorías activas controladas; fuzzing autorizado; análisis de tokens; comparación de mensajes; codificación/decodificación; plugins; proyectos persistentes; generación de informes.

Debe estar especialmente preparada para: Kali Linux; OWASP Juice Shop; PortSwigger Web Security Academy; DVWA; WebGoat; laboratorios CTF; localhost; entornos de desarrollo; sistemas donde el usuario tenga autorización explícita.

La aplicación NO debe estar diseñada para ataques furtivos, evasión, propagación, malware, robo de credenciales, explotación destructiva ni acciones fuera de alcance.

## 2. Principios del proyecto

Prioridades, en este orden:

1. Seguridad.
2. Estabilidad.
3. Exactitud del tráfico HTTP.
4. Arquitectura mantenible.
5. Buen rendimiento.
6. Excelente UX.
7. Extensibilidad.
8. Compatibilidad.
9. Apariencia profesional.

No quiero una demo. No quiero una interfaz falsa con botones que todavía no hacen nada sin indicarlo. No quiero funciones simuladas. Toda función visible deberá: funcionar; estar claramente marcada como experimental; o estar desactivada hasta ser implementada.

## 3. Stack recomendado

**Core:** Rust. El motor de networking debe estar separado completamente de la interfaz gráfica. Arquitectura asíncrona. Runtime: Tokio o alternativa moderna equivalente.

**HTTP:** soporte HTTP/1.1; HTTP/2; preparar arquitectura para HTTP/3 posteriormente.

**TLS:** implementación segura utilizando bibliotecas Rust maduras; generación de certificados únicamente para la interceptación local autorizada; CA local propia; almacenamiento seguro de claves.

**WebSockets:** soporte completo de captura y visualización.

**UI:** Tauri + React + TypeScript. La interfaz NO debe parecerse visualmente a Burp. Diseñar una UX moderna estilo IDE, DevTools, aplicaciones profesionales de debugging y herramientas modernas de desarrollo.

**Persistencia:** SQLite. Utilizar migraciones de base de datos.

**Configuración:** archivos independientes para configuración global, configuración de proyecto, scope, reglas del proxy, scanner, rate limits, plugins.

## 4. Arquitectura

Evita crear un monolito. Estructura aproximada:

```
chosproxy/
├── apps/desktop/
├── crates/ (core, proxy, http, tls, websocket, repeater, crawler, scanner,
│            intruder, decoder, comparer, sequencer, sitemap, sessions,
│            projects, plugins, reporting, storage)
├── packages/ui/
├── docs/  tests/  fixtures/  scripts/
```

No es obligatorio usar exactamente esta estructura si existe una arquitectura mejor. Justifica cambios importantes.

## 5. Proxy HTTP

Implementar un proxy real. Listeners configurables (`127.0.0.1:8080` y adicionales). Funciones: iniciar; parar; reiniciar; seleccionar interfaz; seleccionar puerto; permitir/deshabilitar conexiones remotas; autenticación opcional; reglas por hostname; reglas por protocolo; filtros.

Registrar: timestamp; protocolo; método; URL; host; puerto; status; MIME; tamaño request; tamaño response; duración; IP; TLS; HTTP version.

## 6. Intercept

Modo Intercept ON/OFF. Interceptar requests y responses. Acciones: Forward; Drop; Edit; Send to Repeater; Send to Comparer; Send to Organizer; Decode selection; Copy as raw; Copy URL; Copy headers.

Reglas: Intercept only in-scope requests; Intercept requests with parameters; Intercept responses with specific MIME; Ignore static assets.

## 7. HTTPS MITM local

Implementar interceptación HTTPS para tráfico autorizado. Al primer inicio: generar CA local propia; generar clave privada segura; explicar al usuario qué es; ofrecer exportar el certificado público; proporcionar instrucciones para confiar en él dentro del navegador de laboratorio.

Generar certificados temporales por host cuando sea necesario. Nunca transmitir la clave privada. Nunca incluir una CA global preinstalada. Nunca reutilizar claves entre instalaciones. Botones: **Regenerate Local CA** y **Export CA Certificate**.

## 8. HTTP History

Tabla potente. Columnas: ID; time; method; host; path; status; length; MIME; protocol; TLS; duration; source; notes.

Filtros: scope; status; MIME; method; domain; extension; WebSocket; search; regex.

Panel inferior `Request | Response | Inspector`, vistas `Pretty | Raw | Hex | Render` cuando sea aplicable.

## 9. Inspector

Parser estructurado para: query parameters; path parameters; headers; cookies; body parameters; JSON; XML; multipart; URL encoded; authorization headers; JWT; content type.

Los cambios realizados mediante Inspector deben modificar correctamente el mensaje HTTP original. Actualizar automáticamente Content-Length, encoding y otros metadatos necesarios.

## 10. Repeater

Una de las herramientas principales. Permitir: múltiples pestañas; nombrar pestañas; duplicarlas; historial; raw HTTP editing; pretty editing; Inspector; enviar request; cancelar; mostrar timing; tamaño; status; redirects. Atajo `Ctrl/Cmd + Enter` para enviar. Permitir abrir requests desde cualquier parte de la aplicación. Historial por pestaña para comparar versiones anteriores.

## 11. Site Map / Target

Construir automáticamente el árbol Host → rutas. Registrar: endpoints; métodos; parámetros; status; MIME; origen; primera aparición; última aparición. Diferenciar: navegador; crawler; proxy; repeater; scanner. Marcar elementos: Interesting; Reviewed; Ignore; Favorite.

## 12. Scope

Reglas de inclusión y exclusión por: esquema; hostname; subdominio; puerto; ruta; regex. Visualizar siempre claramente si una petición está IN SCOPE u OUT OF SCOPE. Las herramientas automatizadas deberán estar restringidas por defecto al scope.

## 13. Crawler

Crawler modular. Descubrir: links; forms; scripts; endpoints; redirects; recursos; referencias en HTML; URLs detectadas en JavaScript cuando sea razonablemente posible. Respetar: scope; profundidad; número máximo de requests; concurrencia; rate limit; timeout. Modos: Passive discovery; Active crawl. No ejecutar automáticamente acciones peligrosas (eliminar cuentas, realizar compras, borrar recursos, cerrar sesiones, cambios irreversibles).

## 14. JavaScript y SPA

Preparar arquitectura para React, Vue, Angular, Next.js, SPA. Descubrir rutas y endpoints observados en tráfico y JavaScript. Posteriormente, navegador automatizado aislado para crawling dinámico.

## 15. APIs

Analizar REST; JSON APIs; GraphQL; OpenAPI; Swagger; SOAP cuando corresponda. Importar OpenAPI JSON, OpenAPI YAML, Postman Collections. Mostrar estructura de endpoints.

## 16. WebSockets

Capturar handshake, mensajes enviados y recibidos. Mostrar timestamp; direction; opcode; length; content. Permitir: pausar; inspeccionar; reenviar manualmente en entornos autorizados; buscar; filtrar; exportar.

## 17. Decoder

URL encode/decode; Base64; Base64 URL; Hex; HTML entities; Unicode escapes; gzip; deflate; JWT inspection; hashes como utilidades de transformación/análisis. Transformaciones encadenadas (p. ej. URL Decode → Base64 Decode → Pretty JSON).

## 18. Comparer

Comparar request/request, response/response, texto/texto. Modos: byte diff; word diff; line diff; structured HTTP diff. Resaltar headers añadidos/eliminados, parámetros modificados, cambios de body, diferencias de status y de tamaño.

## 19. Sequencer

Herramienta estadística para tokens de aplicaciones autorizadas: longitud; distribución; entropía estimada; frecuencia; repetición; patrones básicos. Explicar visualmente los resultados. No presentar una conclusión absoluta de "seguro/inseguro" basándose en una única métrica.

## 20. Organizer

Guardar requests/responses interesantes. Campos: nombre; notas; tags; estado; severidad manual; URL; request; response. Estados: Todo; Investigating; Interesting; Confirmed; False Positive; Done.

## 21. Logger

Registro central del tráfico generado por Proxy, Repeater, Crawler, Scanner, Intruder y plugins, con filtros. Cada petición automatizada debe indicar qué módulo la produjo.

## 22. Análisis pasivo

Scanner pasivo que detecta, sin modificar tráfico ni explotar: cookies sin Secure/HttpOnly; SameSite incorrecto o ausente; headers de seguridad ausentes; mixed content; información técnica expuesta; respuestas excesivamente verbosas; CORS sospechoso; caching potencialmente inseguro; formularios sensibles; JWT con configuraciones llamativas; endpoints potencialmente interesantes; versiones de software expuestas.

Cada finding: Title; Severity; Confidence; Host; URL; Evidence; Description; Why it matters; How to verify manually; Remediation; References. Evitar falsos positivos.

## 23. Active Audit Engine

Framework de auditoría activa para laboratorios y sistemas explícitamente autorizados. **Desactivado por defecto.** Antes de activarlo exigir: target definido; scope; confirmación de autorización; límites de velocidad. Priorizar técnicas controladas y no destructivas.

NO implementar: malware; persistencia; evasión; credential stuffing; password spraying; ataques destructivos; borrado de datos; DoS; explotación automática que obtenga control del sistema; técnicas para evitar sistemas de detección. La arquitectura debe permitir checks defensivos adicionales mediante plugins.

## 24. Findings

Panel Issues. Columnas: severity; confidence; type; host; path; source; status. Severity: Info, Low, Medium, High, Critical. Confidence: Tentative, Firm, Certain. Un finding puede almacenar múltiples evidencias. Deduplicar findings equivalentes.

## 25. Intruder / Fuzzer

Motor genérico para pruebas repetitivas autorizadas con posiciones (`username=§value§`): single position; multiple positions; listas personalizadas; números; secuencias; diccionarios aportados por el usuario. Tabla: request number; payload; status; length; duration; error; match. Rate limiting; concurrency; pause; resume; cancel; retry policy. No proporcionar listas orientadas a credenciales reales ni automatización para password spraying/credential stuffing.

## 26. Match & Extract

Grep Match y Grep Extract: status text; header; regex; JSON path; HTML selector.

## 27. Session handling

Administrar cookies; headers; tokens; variables (`{{csrf_token}}`, `{{session_cookie}}`, `{{base_url}}`). Preparar arquitectura para macros autorizadas que mantengan sesiones activas.

## 28. Browser

Lanzar navegador aislado con perfil temporal y proxy configurado automáticamente, sin modificar el navegador habitual. Perfil independiente; proxy automático; CA del proyecto; borrar perfil; abrir URL; seleccionar scope desde la URL actual.

## 29. Search

Búsqueda global en URLs, requests, responses, headers, bodies, findings, notes, site map. Texto; case sensitive; regex.

## 30. Command palette

`Ctrl/Cmd + K`: Open Repeater; Toggle Intercept; New Project; Start Proxy; Search; Open Settings; Export Report.

## 31. Dashboard

Proxy status; requests capturadas; hosts; endpoints; WebSockets; findings; tareas; crawler status; active audit status; rate; errores. Información accionable, no gráficas inútiles.

## 32. Task system

Estados: Queued; Running; Paused; Completed; Failed; Cancelled. Aplicable a crawls, audits, imports, exports, fuzzing, análisis. Cada tarea muestra progreso, duración, requests, errores, logs.

## 33. Live analysis

Tareas automáticas sobre tráfico manual (p. ej. passive analyze all in-scope Proxy traffic; add in-scope traffic to Site Map; analyze Repeater traffic). No lanzar active audits automáticamente de fábrica.

## 34. Projects

Guardar: HTTP history; site map; findings; repeater tabs; organizer; scope; settings; notes; tasks; WebSocket history. Tipos: Temporary Project, Persistent Project. Acciones: New; Open; Save; Save As; Export; Backup.

## 35. Base de datos

SQLite; schema normalizado y eficiente; grandes cantidades de requests sin bloquear la UI; no almacenar bodies gigantes en memoria; streaming, compresión, blob storage, paging, lazy loading; migraciones versionadas.

## 36. Plugin system

Según permisos, un plugin podrá recibir eventos HTTP; analizar requests y responses; añadir findings, menús, tabs, transforms, passive checks. Ejecución aislada en la medida de lo posible. Permisos declarados (`network.read`, `network.send`, `project.read`, `project.write`, `ui.extend`, `scanner.passive`). No conceder permisos automáticamente.

## 37. Plugin SDK

Documentación y ejemplo mínimo `HelloPlugin`: recibe tráfico, detecta una cabecera concreta y añade un finding informativo.

## 38. Reporting

Exportar findings en HTML, JSON, Markdown (posteriormente PDF). Informe: Executive Summary; Target; Scope; Methodology; Findings; Evidence; Severity; Remediation; Technical Details. Seleccionar qué findings incluir.

## 39. UI general

Tema oscuro por defecto para Kali; también Light y System. Layout: barra superior (proyecto, estado del proxy, scope, búsqueda), navegación lateral, workspace principal, barra inferior (tareas, errores, estado de red). No seguir el wireframe de forma ciega si existe una UX mejor.

## 40. HTTP message editor

Componente crítico y reutilizable (Proxy, Repeater, Logger, Scanner, Organizer, Site Map): syntax highlighting; line numbers; raw HTTP; pretty JSON; pretty XML; word wrap; search; editable/read-only; context menu; copy; send to tools. Debe funcionar con payloads binarios.

## 41. Rendimiento

Miles o decenas de miles de mensajes sin degradar la UI: virtual scrolling; pagination; indexing; background workers; async I/O; bounded queues. Evitar cargar todo el proyecto en RAM.

## 42. Seguridad interna

Threat modeling del propio ChosProxy: CA privada; archivos de proyecto; plugins; imports; HTML rendering; scripts; archivos maliciosos; parsing; deserialización; command injection; path traversal. Nunca ejecutar JavaScript recibido en responses dentro del contexto privilegiado de Tauri. Aislar el rendering de contenido no confiable.

## 43. Privacidad

Todo local por defecto. Sin analytics, sin telemetría, sin subida automática de datos, sin nube obligatoria. Integraciones externas solo opt-in explícito.

## 44. Logging

Niveles ERROR, WARN, INFO, DEBUG, TRACE. No registrar por defecto contraseñas, cookies completas, Authorization, claves privadas ni secretos. Redacción automática.

## 45. Testing

Tests desde el principio. Unit: parsers, HTTP, scope matching, decoder, storage. Integration: proxy, HTTPS, Repeater, WebSockets, persistencia de proyectos. End-to-end con servidores locales controlados y fixtures locales; sin depender de webs externas.

## 46. Laboratorio de desarrollo

Servidor vulnerable mínimo dentro del repo, solo para tests locales: HTTP; HTTPS; cookies; headers; forms; JSON; redirects; WebSockets. Escucha solo en localhost por defecto.

## 47. CI/CD

GitHub Actions: format, lint, unit tests, integration tests, frontend tests, build. No publicar releases automáticamente hasta que los tests pasen.

## 48. Kali Linux

Plataforma principal. Paquetes `.deb` y AppImage. Entrada en el menú de aplicaciones, categoría Web Application Analysis. Comando `chosproxy`. Arquitectura x86_64; preparar ARM64.

## 49. Instalación

`sudo dpkg -i chosproxy_VERSION_amd64.deb` o AppImage, sin configuración manual absurda. Primer inicio: Welcome → crear proyecto temporal/persistente → configurar proxy → generar CA local → abrir navegador → empezar a interceptar.

## 50. Documentación

README.md; CONTRIBUTING.md; SECURITY.md; ARCHITECTURE.md; DEVELOPMENT.md; PLUGIN_SDK.md; THREAT_MODEL.md; ROADMAP.md; CHANGELOG.md. El README debe explicar que solo debe usarse en sistemas propios o con autorización.

## 51. Roadmap

No construir todo a la vez. M0 Foundation · M1 HTTP Proxy · M2 HTTPS · M3 Intercept · M4 Repeater · M5 Target (scope, site map, search, inspector) · M6 Utilities (Decoder, Comparer, Organizer) · M7 WebSockets · M8 Projects · M9 Passive Scanner · M10 Crawler · M11 Fuzzer · M12 Sequencer · M13 Active Audit Framework · M14 Plugins · M15 Reporting · M16 Performance & Hardening · M17 1.0.

## 52. Definición de Done

Un milestone solo está terminado cuando: compila; funciona; tiene tests; no rompe milestones anteriores; está documentado; los errores importantes están gestionados; la UI tiene estados loading/error/empty; no existen botones falsos; lint pasa; CI pasa.

## 53. Política de cambios

Antes de una decisión arquitectónica difícil de revertir: analizar alternativas; escribir la decisión; ventajas; inconvenientes; seleccionar una. Guardar como ADR en `docs/adr/`.

## 54. No hagas esto

Un único archivo gigantesco; `unwrap()` indiscriminados; secretos hardcodeados; TLS inseguro por comodidad; desactivar validaciones; dependencias abandonadas; código duplicado; `TODO` eternos; componentes frontend gigantes; queries SQLite sin índices; toda la DB en RAM; blocking I/O en UI; botones sin implementar; mocks como implementación final; código ofensivo oculto; telemetría.

## 55. Calidad del código

Rust: idiomático; errores tipados; documentación pública; módulos pequeños; tests. TypeScript: strict; evitar `any`; componentes reutilizables; separación UI/state/services. Formatting y linting automáticos.

## 56. Git

Commits pequeños y claros (`feat(proxy): add HTTP listener`, `fix(proxy): handle chunked responses`, …). Nada de un commit gigante.

## 57. Objetivo de UX

Instalar → abrir → crear proyecto → Open Browser → navegar → ver tráfico → interceptar → modificar → enviar a Repeater → analizar la respuesta, sin un manual de 40 páginas.

## 58. Diferenciación

Explicaciones educativas de cada finding (qué, por qué, dónde, qué lo demuestra, cómo comprobarlo, cómo arreglarlo); Visual Request Flow (Browser → Proxy → Modification → Server → Response); diff automático en Repeater; timeline de requests relacionadas; scope siempre visible.

## 59. Futuro

Preparar (sin implementar aún): HTTP/3; gRPC; mobile proxying; Android; iOS; MCP; CLI; headless mode; scripting API; collaborative projects; optional AI assistance.

## 60. IA futura

No enviar automáticamente tráfico a proveedores externos. Modos: Local AI; External AI (explícitamente habilitada); Disabled. Funciones: explicar request/response/finding; resumir diferencias; sugerir pasos manuales; generar documentación. La IA no ejecuta acciones ofensivas sin intervención explícita del usuario.

## 61. Compatibilidad

Primero Kali Linux x86_64; después Debian, Ubuntu, Windows, macOS ARM64 (y x86_64 si sigue siendo relevante). Core independiente del sistema operativo.

## 62. Licencia

Comparar MIT, Apache-2.0, GPLv3, AGPLv3 antes de elegir; documentar implicaciones.

## 63. Resultado final deseado

Reconocer aplicación → capturar tráfico → entender endpoints → modificar requests → repetir pruebas → comparar respuestas → analizar comportamiento → detectar posibles problemas → documentar findings → generar informe. Todo en una única aplicación.

## 64. Instrucciones para el agente

Analizar la especificación; inspeccionar el repositorio; crear ARCHITECTURE.md, ROADMAP.md y ADRs iniciales; diseñar el workspace; identificar riesgos; definir interfaces; crear tests base; implementar únicamente M0; verificar; continuar con M1. Tras cada milestone: build, tests, lint, corregir errores, actualizar documentación, revisión de seguridad, revisar regresiones. Ante un problema: causa raíz, corrección, test de regresión.

## 65. Primer objetivo funcional

Kali Linux → ChosProxy → Browser → OWASP Juice Shop local: iniciar ChosProxy; abrir un navegador aislado; navegar al laboratorio; capturar HTTP/HTTPS; ver request y response; activar Intercept; modificar una petición; enviarla; mandarla a Repeater; modificarla allí; repetirla; comparar responses; almacenarla en el proyecto.

## 66. Regla final

Prioriza una herramienta pequeña que funcione perfectamente antes que una enorme llena de funciones incompletas. Cada paso debe ser utilizable por sí mismo.
