# ChosProxy

**Un entorno local para inspeccionar, modificar y entender el tráfico de aplicaciones web.**

ChosProxy combina un proxy HTTP(S) con herramientas de pruebas de seguridad. Utiliza un motor Rust y una interfaz Tauri + React, con código, diseño e identidad propios.

[Herramientas](#herramientas) · [Instalación en Kali](#instalación-en-kali) · [Desde el código fuente](#desde-el-código-fuente) · [Primeros pasos](#primeros-pasos) · [Documentación](#documentación)

> Utilízalo con sistemas propios o con autorización expresa: laboratorios locales, CTF, entornos de desarrollo y pruebas acordadas. Consulta [SECURITY.md](SECURITY.md).

## Estado y alcance

Proyecto en desarrollo. El repositorio incluye herramientas de análisis, automatización e integración; la [hoja de ruta](ROADMAP.md) recoge su evolución. El listado de funciones describe el código documentado y no sustituye las comprobaciones en cada plataforma.

El trabajo se realiza localmente. Las funciones de IA permiten usar un modelo local o un proveedor externo elegido de forma explícita; revisa el contexto y la configuración antes de compartir datos.

## Herramientas

| Área | Funciones |
| --- | --- |
| **Proyectos** | Proyectos temporales y persistentes en SQLite; cifrado SQLCipher, recuperación tras fallos, inspección de solo lectura y registro de auditoría. |
| **Proxy HTTP(S)** | Varios puntos de escucha, loopback por defecto, HTTP/2 hacia el navegador y modo invisible. Retransmisión HTTP/1.1 que conserva los bytes, conexiones persistentes, fragmentación y cambios de protocolo. |
| **Interceptación** | Pausar, editar, reenviar o descartar peticiones y respuestas. Reglas y sustituciones automáticas sobre líneas, cabeceras y cuerpos. |
| **Red y sesiones** | Proxies HTTP/SOCKS5 por destino, nombres de host alternativos, tiempos de espera, credenciales cifradas, Basic/Digest y macros de inicio de sesión. |
| **Historial** | Tabla virtualizada, filtros, ordenación, vistas Pretty/Raw/Hex/Inspector, edición JSON y copia como curl, Python o fetch. |
| **Repeater** | Pestañas y grupos persistentes, envíos individuales o agrupados, cancelación, cookies, tiempos, historial, redirecciones, notas, HTTP/2 y pruebas de carrera de un solo paquete. |
| **Análisis pasivo** | 25 comprobaciones sobre peticiones, JavaScript y relaciones entre mensajes, con explicaciones y reglas para descartar falsos positivos. |
| **Exploración y auditorías** | Rastreo GET dentro del alcance, exploración con navegador, análisis JavaScript, tareas pausables y auditoría pasiva que continúa al reabrir el proyecto. |
| **Auditoría activa** | Desactivada por defecto. Exige objetivo, alcance, autorización por ejecución y límites de peticiones. Incluye comprobaciones XSS, SQLi, redirecciones, CORS y OAST, además de scripts propios. |
| **Fuzzer** | Posiciones automáticas en consulta, formularios, cookies, JSON, multipart y XML; cuatro modos de ataque, procesamiento de cargas, búsqueda y extracción, condiciones de parada y CSV. |
| **Sequencer** | Captura de tokens y análisis explicado de su aleatoriedad. |
| **API y GraphQL** | Importación OpenAPI/Swagger, Postman y WSDL hacia Repeater/Fuzzer; introspección, exploración de esquema y construcción de consultas GraphQL. |
| **Extensiones** | Complementos con permisos, publicadores de confianza y firma; scripts aislados para comprobaciones, filtros, columnas, acciones y reescrituras. |
| **DOM y OAST** | Análisis de origen a destino en el navegador de laboratorio; interacciones DNS/HTTP(S) vinculadas a sus peticiones mediante ChosOAST local o alojado por el usuario. |
| **Organización** | Colecciones, notas, descubrimiento de contenido, detección de soft-404, prueba CSRF, FrameLab, búsqueda de referencias, capturas del mapa y programación de tareas. |
| **Recursos** | Límites compartidos de peticiones entre rastreador, Fuzzer, Sequencer, Repeater y complementos; reducción del ritmo ante respuestas 429/503. |
| **ChosAI y ChosAgent** | Explicaciones, análisis de hallazgos e informes; agente con permisos Deny/Ask/Allow aplicados por el motor, alcance controlado y registro de actividad. |
| **Automatización** | API local con token, flujo de eventos, descripción OpenAPI y `chosproxy-cli` para ejecución sin interfaz. |

Incluye navegador de laboratorio con perfil temporal, autoridad certificadora local, temas claro/oscuro, paleta de comandos y atajos configurables. Las funciones M31b añaden respuestas de IA progresivas, NTLMv2, almacén de claves, receptor SMTP para OAST y ampliaciones de API y complementos.

## Instalación en Kali

Para Kali Linux x86_64, si dispones del paquete generado:

```bash
sudo apt install ./chosproxy_1.0.0_amd64.deb
```

Abre **ChosProxy** desde el menú de aplicaciones o ejecuta `chosproxy`. También puede utilizarse una AppImage con permiso de ejecución. Los paquetes se construyen con `scripts/build-kali.sh`; consulta los artefactos disponibles en la integración continua.

## Desde el código fuente

Requisitos documentados: Rust estable 1.89 o posterior, Node.js 22 o posterior y pnpm. En Linux se necesitan las dependencias de WebKitGTK:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libxdo-dev build-essential
pnpm install
pnpm dev
```

## Primeros pasos

1. Pulsa **Start temporary project** para abrir un proyecto temporal.
2. Pulsa **Open browser** para iniciar el navegador de laboratorio.
3. El proxy escucha por defecto en `127.0.0.1:8080`.
4. Navega por tu laboratorio y revisa las peticiones en **Proxy → HTTP history**.

Se utiliza Chromium/Chrome o Firefox instalado con un perfil temporal configurado para el proxy. Si no hay navegador compatible en Kali:

```bash
sudo apt install chromium
```

### HTTPS y certificados

En el primer arranque se genera una autoridad certificadora local. Puedes consultarla en **Proxy → HTTPS & browser**. Su clave privada permanece en el equipo.

El perfil temporal del navegador de laboratorio acepta sus certificados. Para otro navegador dedicado a pruebas, abre `http://chosproxy/` a través del proxy y sigue las instrucciones de importación.

### Laboratorio con Juice Shop

```bash
docker run --rm -p 127.0.0.1:3000:3000 bkimminich/juice-shop
```

Abre `http://localhost:3000` en el navegador de laboratorio.

### Servidor de práctica integrado

Servidor local, limitado a loopback, con HTTP y HTTPS mediante certificado autofirmado:

```bash
pnpm lab -- --port 9090 --tls-port 9443
```

## Documentación

| Guía | Contenido |
| --- | --- |
| [Especificación](docs/PROJECT_SPEC.md) | Objetivos y alcance del producto; [ampliación de requisitos](docs/PROJECT_SPEC_ADDENDUM_PARITY.md). |
| [Matriz de capacidades](docs/BURP_PARITY_MATRIX.md) | Comparativa documentada con la suite de referencia. |
| [Arquitectura](ARCHITECTURE.md) | Módulos, flujos de datos e interfaces. |
| [Desarrollo](DEVELOPMENT.md) | Compilación, pruebas, estructura y convenciones. |
| [Hoja de ruta](ROADMAP.md) | Hitos y criterios de finalización. |
| [Modelo de amenazas](THREAT_MODEL.md) | Seguridad de la propia herramienta. |
| [IA y agente](docs/AI.md) | Proveedores, contexto y permisos. |
| [Red](docs/NETWORK.md) y [archivos de proyecto](docs/PROJECT_FILES.md) | Conexión, persistencia y privacidad. |
| [API](docs/API.md) y [SDK de complementos](PLUGIN_SDK.md) | Automatización y extensiones. |
| [Scripts](docs/SCRIPTS.md) | Comprobaciones y acciones personalizadas. |
| [Inspector DOM](docs/DOM_INSPECTOR.md) y [OAST](docs/OAST.md) | Análisis en navegador e interacciones externas. |
| [Seguridad](SECURITY.md) y [contribuciones](CONTRIBUTING.md) | Uso previsto y participación. |
| [Decisiones de arquitectura](docs/adr/) | Motivos de las decisiones técnicas. |

## Licencia

La licencia está pendiente de elección por el propietario. Las opciones y la recomendación están documentadas en [ADR-0008](docs/adr/0008-license.md). Que el repositorio sea público no concede por sí solo una licencia de reutilización.
