# Prompt de implementación: M31 A/C/F y M13

Preparado el 14/11/2026 tras revisar documentación y código. Los cuatro
apartados están pendientes; este documento no certifica su implementación.
Copia el bloque siguiente en la IA que vaya a trabajar en el repositorio.

```text
Trabaja sobre el repositorio actual de ChosProxy. Implementa hasta terminar
M31 A, C y F, y después M13, en ese orden. Esta petición autoriza los cuatro
apartados: continúa entre ellos sin volver a pedir una decisión de alcance.
No te quedes en un plan ni marques como terminado algo que solo compila.

CONTEXTO Y FORMA DE TRABAJO

1. Lee STATUS.md, ROADMAP.md, ARCHITECTURE.md, DEVELOPMENT.md,
   docs/PROJECT_SPEC.md §23, docs/PROJECT_SPEC_ADDENDUM_PARITY.md §117–122,
   docs/BURP_PARITY_MATRIX.md y los ADR 0035, 0036, 0040 y 0041.
   Verifica el estado actual del código antes de cambiarlo: esta revisión
   corresponde al 14/11/2026 y puede haber cambios posteriores.
2. Revisa git status y conserva los cambios ajenos. El flujo del proyecto
   pide trabajar en main, sin stashes ni ramas nuevas, con commits pequeños
   por paso terminado. No hagas push ni tags. Atribuye los commits con
   veracidad al autor/modelo que realiza el trabajo.
3. Las antiguas prohibiciones documentales de retomar estos apartados se
   han sustituido por su estado real: pendientes y seleccionados por mí.
   Implementa conforme a la especificación vigente. Conserva los controles
   funcionales del producto: scope, permisos, límites, cifrado y protección
   de datos. Esta tarea no cambia el catálogo ni los permisos de ChosAgent.
4. Completa motor Rust → comandos Tauri registrados → bindings ts-rs →
   services/ipc.ts → estado y UI React. Respeta los patrones existentes,
   errores tipados y estados de carga, vacío y error. Nada de botones sin
   implementación, mocks en producción ni TODOs como sustituto del trabajo.
5. Escribe primero una lista verificable de tareas en STATUS.md y actualízala
   al cerrar cada paso. Inspecciona el espacio disponible antes de builds
   grandes: este repositorio está en una carpeta sincronizada con iCloud.
   No borres proyectos ni ejecutes limpiezas extensas automáticamente.

M31 A — RECUPERACIÓN TRAS CIERRES INESPERADOS

Base real que debes reutilizar:
- crates/storage ya tiene OpenOptions.read_only, StorageError::ReadOnly,
  ProjectDb::is_read_only y ProjectDb::quick_check; hay pruebas en storage.
- El ciclo de proyectos y shutdown está en crates/app/src/engine.rs y
  engine/project_ops.rs. Las tareas viven en engine/task_ops.rs y las
  tareas pasivas persistentes en engine/live_ops.rs. Revisa temp_ops.rs.
- Falta la integración de recuperación en motor, IPC y arranque de la UI.

Implementa:
- Un registro de sesión versionado en el directorio de datos, actualizado
  al abrir/cerrar un proyecto y al cambiar las tareas relevantes, con
  escritura atómica. Guarda solo metadatos necesarios, nunca claves,
  contraseñas, cuerpos de peticiones ni tokens.
- Distingue una instancia todavía activa de un cierre inesperado. Trata
  registros corruptos, PID reutilizado, proyectos movidos y varias instancias
  sin apropiarte de otra sesión. Decide y documenta cómo reconocer una
  instancia, teniendo en cuenta que la configuración puede compartirse.
- recovery_status y operaciones restore/open_read_only/discard, con nombres
  adaptados a las convenciones del proyecto. El arranque ofrece Restore,
  Open read-only y Discard recovery, con mensajes claros y opción de cancelar.
- Restore comprueba integridad antes de abrir y pide contraseña cuando
  corresponda. Una contraseña incorrecta o una BD dañada no pierde el
  registro ni cierra otro proyecto que ya esté abierto.
- Las tareas interrumpidas quedan representadas de forma compatible con
  TaskStatus. Nunca relances automáticamente tareas que generan tráfico.
  Mantén el comportamiento existente de las tareas puramente pasivas.
- Read-only debe cumplirse en todo el motor, no solo ocultando botones:
  sin escritura, migraciones, listeners, envíos ni tareas con efectos.
  Permite lectura e inspección, indica el modo en una banda visible y
  comprueba que abrir así no modifica el archivo ni crea escrituras laterales.
- Coordina recuperación y limpieza: protege los temporales recuperables;
  descartar el aviso no equivale a borrar el proyecto. Cualquier eliminación
  opcional de un temporal huérfano debe identificarlo y confirmarse.
- Un cierre limpio retira únicamente el registro de esta sesión.

Prueba registros obsoletos y corruptos, otra instancia activa, interrupción
de tareas, modo read-only, rutas inexistentes, cifrado, cancelación y
coordinación con la limpieza. Usa directorios aislados y fallos simulados.

M31 C — EXPORTACIONES CIFRADAS UTILIZABLES EN LA APP

No reconstruyas la criptografía. Ya existen:
- crates/storage/src/sealed.rs: seal/open/is_sealed, PBKDF2-HMAC-SHA256
  600 000 iteraciones y AES-256-GCM con cabecera autenticada/versionada.
- crates/app/src/engine/share_ops.rs: organizer_export, organizer_import y
  OrganizerShare. La importación usa organizer_insert_all, todo o nada.
- engine/project_ops.rs: export_settings_with,
  preview_settings_import_with y apply_settings_import_with.
- UI: features/organizer/OrganizerView.tsx,
  features/settings/SettingsTransferCard.tsx y state/password.ts.

Completa comandos Tauri, registro, IPC, bindings y UI para exportar una
colección o todo el Organizer, importar, y exportar/importar ajustes con
contraseña. Mantén compatibilidad con los archivos sin cifrar existentes.
Detecta el formato antes de pedir contraseña; reutiliza los diálogos que
encajen y permite reintentar o cancelar sin perder el estado.

Comprueba la semántica de colecciones anidadas: el motor actual filtra por
coincidencia exacta. Define claramente si incluye descendientes y aplícalo
igual en UI, motor y pruebas, sin omitirlos de forma silenciosa.

La vista previa de ajustes debe describir qué cambia antes de aplicar y
seguir usando la validación existente. Importar datos nunca los ejecuta.
Conserva los límites de tamaño/cantidad, la atomicidad y los permisos
privados de los archivos. Evita destruir un archivo anterior si falla una
exportación: escritura temporal y sustitución atómica donde corresponda.
No guardes contraseñas en stores persistentes, logs ni mensajes de error.

Añade integración de motor con round-trip de bytes de request/response,
metadatos y colecciones; contraseña errónea; cabecera/ciphertext alterados;
versión desconocida; archivo truncado; ítem inválido sin inserción parcial;
ajustes cifrados y planos; cancelación y fallos de escritura. Prueba los
flujos de UI con el estilo de pruebas existente, además de las primitivas.

M31 F — SECURITY CENTER Y COBERTURA DEL REGISTRO DE AUDITORÍA

Implementa security_overview() o equivalente con un resultado tipado de
comprobaciones ok/warn/info y destinos de navegación. Obtén el estado real
del motor/configuración; no dupliques las reglas solo en el frontend.

Incluye CA y ubicación de su clave sin mostrarla; listeners expuestos y
autenticación; API de automatización; plugins y permisos concedidos;
política de firmas; modo/proveedor ChosAI; permisos ChosAgent; proxies
upstream; modo OAST y conexiones externas configuradas; excepciones de
verificación TLS; cifrado del proyecto; temporales y limpieza. Distingue
configurado de activo, ausente de error y advertencia de vulnerabilidad.

Añade la vista Security & Privacy con navegación que funcione hacia los
ajustes implicados. Debe funcionar también sin proyecto abierto, indicando
como no disponibles las comprobaciones que dependan de un proyecto.
Es una vista local: obtener el resumen no envía tráfico ni prueba servicios.

Antes de añadir eventos, inventaría los que ya se emiten y su persistencia.
Completa §122: concesión/revocación de permisos de plugins; inicio y fin
de scans/crawls y demás tareas de tráfico, con cancelación o error;
aprobaciones/rechazos de ChosAgent; recuperación; cifrado/descifrado/rekey;
exportaciones y cambios de scope/findings que falten. No dupliques eventos
ya emitidos. Define qué registrar si todavía no hay proyecto escribible,
sin forzar escrituras en una BD abierta en read-only ni perder fallos.

Ya existe AuditCard en features/project/ProjectView.tsx y audit_log en
motor, storage, Tauri e IPC. Amplíalos: filtros de acción, fechas y texto,
orden estable y paginación/límite visible. No crees un segundo registro
incompatible ni confundas el audit log del proyecto con el event-log.jsonl
global o el AI activity log. Mantén las garantías append-only existentes.

Prueba cada comprobación con configuraciones reales aisladas, cada familia
de eventos, ausencia de secretos y lectura del registro al reabrir.

M13 — FRAMEWORK DE AUDITORÍA ACTIVA

Implementa §23 como capacidad real del scanner, con motor, ejecución,
evidencias, configuración, UI y pruebas. Hoy no está implementado.

Reutiliza:
- crates/scanner/src/insertion.rs: PointKind/PointEncoding,
  insertion_points, replace y encode; fingerprint.rs para deduplicación.
- engine/scan_ops.rs: ScanConfig, ScanLaunch, fases, resumen y librería.
- engine/task_ops.rs: controles de tareas y ensure_in_scope;
  engine/session_ops.rs y pool_ops.rs para envíos y límites compartidos.
- Findings y findings_record_auto: evidencias, severidad/confianza,
  supresiones y deduplicación. source scanner ya se usa en otros envíos.
- features/tasks/ScanDialog.tsx, CheckPicker.tsx y ScanDashboard.tsx.
- crates/lab y los tests de integración locales para los escenarios.

Diseño y ejecución:
- Define un contrato tipado de active checks: id estable, descripción,
  precondiciones, puntos de inserción compatibles, presupuesto, generación
  de pruebas y evaluación de evidencias. Separa análisis puro de envío.
- Una fase active audit opcional, desactivada por defecto; configuraciones
  antiguas conservan su comportamiento. Lanzar requiere target, scope
  explícito, autorización y límites, comprobados también desde el motor.
  Valida igualmente lanzamientos programados y cualquier entrada API/IPC.
- Selección de checks y puntos; estimación previa del presupuesto; límites
  por scan/host/check, concurrencia, timeout, tamaño de respuesta, reintentos
  y back-off. Comparte los Resource Pools y las sesiones existentes.
- Revalida scope en cada envío y redirect. Los cambios de scope durante
  una tarea deben surtir efecto. Una baseline válida no autoriza otros hosts.
- Usa peticiones baseline y controles comparables; respeta la codificación
  del punto y actualiza framing/Content-Length correctamente. No introduzcas
  un segundo cliente HTTP que omita sesiones, proxy o historial.
- Pausa, reanuda y cancela tanto el trabajo en curso como la cola y sus
  esperas; libera permisos/slots. No prosigas enviando tras detenerse.
- Guarda el tráfico como scanner, asociado al scan/check/punto, con progreso,
  motivo de omisión y errores visibles. Integra la fase en el dashboard.
- Findings reproducibles: baseline, petición de prueba, respuesta,
  diferencias, punto, check, confianza y explicación. Un timeout, 500 o
  una cadena reflejada no bastan para afirmar explotación confirmada.

Catálogo inicial entregable:
- Reflexión de entrada y análisis de contexto para XSS reflejado, con
  marcadores inocuos. Sin ejecución demostrada, informa reflexión/contexto
  y confianza tentativa, no XSS confirmado.
- Inyección SQL mediante comparaciones booleanas controladas y señales de
  error corroboradas, sin extraer tablas/datos ni modificar información.
  Maneja respuestas dinámicas, filtros y errores genéricos con controles.
- Redirecciones abiertas mediante inspección de Location, sin seguir a
  terceros; evaluación CORS con origen de prueba y controles de baseline.
- Integración OAST opcional con identificadores únicos y correlación de
  interacciones, únicamente si el usuario configura su entorno autorizado.
  Sin interacción correlacionada, no confirmes SSRF ni otra vulnerabilidad.
  No consultes metadatos de nube, archivos sensibles ni destinos ajenos.
- Diseña y documenta cómo añadir más checks con el contrato anterior.
  No declares familias enteras completas si solo cubres una variante.

El objetivo es probar aplicaciones propias y laboratorios autorizados.
Respeta el alcance no destructivo de la especificación. No incluyas
persistencia, extracción de secretos, evasión, listas de credenciales,
obtención de shell ni pruebas de carga. Métodos/operaciones que puedan
cambiar estado deben quedar excluidos por defecto y requerir selección
explícita del usuario. No amplíes permisos de agentes para implementar M13.

Pruebas positivas y negativas con servidores locales deterministas:
vulnerable frente a corregido; respuestas variables; errores genéricos;
contextos escapados; control negativo SQL; redirects fuera de scope;
cambio de scope; límites; cancelación; sesiones; deduplicación/supresiones;
OAST correlacionado frente a ajeno. Ningún test necesita Internet.
No conviertas evidencias tentativas en confirmadas para hacer pasar tests.

VALIDACIÓN Y CIERRE

- Ejecuta pruebas dirigidas durante cada paso y los controles del proyecto
  antes de cerrar: cargo fmt --all --check; cargo clippy --workspace
  --all-targets -- -D warnings; cargo test --workspace; desde apps/desktop,
  npx tsc -b, npx eslint src, npx prettier --check src y npx vitest run;
  desde la raíz, node scripts/check-bindings.mjs y
  node scripts/matrix-summary.mjs --check. Si falta dist para Tauri, compila
  primero el frontend como indica DEVELOPMENT.md. No reformatees bindings
  generados: regénéralos desde Rust.
- Si cambias el shell/UI compartida, build frontend y check-bundle.mjs.
  Verifica las pantallas y flujos en la app con CHOSPROXY_HOME aislado si el
  entorno permite interacción. Diferencia pruebas automatizadas de las
  comprobaciones manuales que realmente hayas realizado.
- Actualiza STATUS, ROADMAP, CHANGELOG, README, THREAT_MODEL, los documentos
  de usuario implicados y ADR/índice. Documenta M13 y la ampliación de M31,
  incluyendo limitaciones reales y compatibilidad de archivos/configuración.
- Revisa PROJECT-004, ORGANIZER-003, PLATFORM-002, SCANNER-002/005/013/014
  y las demás filas afectadas. Cambia estados solo con implementación y
  evidencia; no atribuyas instrumentación de navegador o paridad completa
  a estos cuatro apartados. Regenera la tabla resumen de la matriz.
- Cierra con archivos cambiados, comportamiento entregado, pruebas con
  resultado, commits y limitaciones. Si el entorno bloquea una comprobación,
  explica el bloqueo concreto y continúa el resto del trabajo verificable.
```
