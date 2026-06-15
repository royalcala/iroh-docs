# iroh-docs — Análisis de Arquitectura

**Repositorio:** `n0-computer/iroh-docs` — Rust crate para documentos key-value multidimensionales con sincronización eficiente.

**Documentos relacionados:**
- [Mapeo de conceptos](mappings.md) — SQL, red, CRDT, claves, API, glosario

## Resumen

`iroh-docs` es un sistema de sincronización de documentos basado en **replicas**. Cada réplica es un espacio de nombres (namespace) que contiene entradas key-value firmadas por dos pares de claves ed25519: el **Author** (autoría) y el **Namespace** (permiso de escritura). Los datos reales (blobs) no se almacenan en la réplica — solo su hash BLAKE3, tamaño y timestamp.

La sincronización entre pares usa **set reconciliation basado en rangos** (paper de Aljoscha Meyer), que compara fingerprints de particiones recursivamente para detectar diferencias.

El modelo de datos y resolución de conflictos sigue el diseño **Willow Protocol** — un protocolo con propiedades CRDT-like (convergencia eventual determinista sin coordinación). Ver sección [Willow y CRDT](#willow-y-crdt).

---

## Estructura del código (`src/`)

### `lib.rs` — Punto de entrada
Re-exporta los módulos públicos y tipos principales (`AuthorHeads`, claves, `SyncOutcome`, `DocTicket`).

### `keys.rs` — Criptografía
Define las claves del sistema:
- **`Author`** / `AuthorId` / `AuthorPublicKey` — identidad del autor
- **`NamespaceSecret`** / `NamespaceId` / `NamespacePublicKey` — identidad del espacio de nombres
- Las entradas se firman con ambas claves (namespace + author).

### `sync.rs` — Core de la réplica
Contiene los tipos fundamentales:
- **`Entry`** = `RecordIdentifier` (namespace + author + key) + `Record` (hash, len, timestamp)
- **`SignedEntry`** = `Entry` + `EntrySignature` (namespace_sig + author_sig)
- **`Replica`** — struct genérico que envuelve un `StoreInstance` y `ReplicaInfo`. Métodos: `insert`, `delete_prefix`, `insert_remote_entry`, `sync_initial_message`, `sync_process_message`
- **`Capability`** — Write (con secret key) o Read (solo public key)
- **`ReplicaInfo`** — estado en memoria de una réplica abierta (capability, subscribers, callbacks)
- **`SyncOutcome`** — resultado de una sincronización (heads recibidos, conteo de entradas)
- **`Event`** — eventos emitidos a subscribers (`LocalInsert`, `RemoteInsert`)
- Validación de entradas: verifica firma, namespace, timestamp futuro máximo (10 min), entrada vacía.

### `ranger.rs` — Algoritmo de Set Reconciliation
Implementación del protocolo de reconciliación basado en rangos:
- **`RangeEntry`** trait — entradas que pueden ser fingerprintadas y agrupadas en rangos
- **`RangeKey`** / **`RangeValue`** traits — keys y valores ordenables
- **`Store`** trait — interfaz de almacenamiento para el algoritmo (get_range, get_fingerprint, put, remove_prefix_filtered, etc.)
- **`Message`** / **`MessagePart`** — mensajes del protocolo (`RangeFingerprint` o `RangeItem`)
- **`process_message`** — procesa mensajes entrantes: compara fingerprints, hace split recursivo si difieren, envía/recibe items
- **`put`** — inserta una entrada aplicando reglas Willow: la entrada debe ser estrictamente más nueva que cualquier prefijo existente, y elimina entradas con prefijo de la nueva.

### `store/` — Persistencia
- **`store.rs`** — traits y tipos: `DownloadPolicy`, `Query`/`QueryBuilder`, `SortBy`, `KeyFilter`, `AuthorFilter`
- **`store/fs.rs`** + **`store/fs/`** — implementación concreta sobre `redb` (embedded K-V store). Soporta modo memoria (`Vec<u8>`) y modo persistente (archivo en disco, típicamente `docs.redb`). Implementa el trait `ranger::Store`.
- **`store/pubkeys.rs`** — trait `PublicKeyStore` para resolver `NamespaceId`/`AuthorId` a claves públicas.

> **Nota:** Todas las réplicas y autores comparten un mismo archivo `docs.redb`. No hay un archivo por documento. Las tablas internas de redb separan los datos por `NamespaceId` en sus claves.

**Tablas internas de redb y cómo se separan por namespace:**

```
docs.redb (UN solo archivo)
├─ NAMESPACES_TABLE:     key=[NamespaceId: 32B]     → (capability_kind, raw_bytes)
├─ RECORDS_TABLE:        key=[NamespaceId|AuthorId|Key] → (timestamp, sigs, len, hash)
├─ LATEST_PER_AUTHOR:    key=[NamespaceId|AuthorId] → (timestamp, key)
├─ NAMESPACE_PEERS:      key=[NamespaceId: 32B]     → (timestamp, peer_id)
└─ DOWNLOAD_POLICY:      key=[NamespaceId: 32B]     → policy
```

Todas las tablas usan `NamespaceId` como **prefijo de la clave** en el B-tree. Como redb ordena lexicográficamente, todos los registros de un mismo namespace son contiguos en el árbol — es eficiente para scans por namespace. Esto significa que **un solo peer con un solo archivo `docs.redb` puede participar en cientos de documentos distintos**, cada uno sincronizando con peers diferentes.

### Modelo de clave compuesta

Cada entrada en una réplica se identifica con un **`RecordIdentifier`** que concatena tres componentes:

```
[namespace_id: 32B] [author_id: 32B] [key: bytes variables]
```

Esto tiene implicancias importantes:

- **Multi-autor sin conflicto:** Dos autores distintos (`alice`, `bob`) pueden escribir a la misma key `"/config"` sin sobrescribirse — son entradas distintas porque el `author_id` difiere.
- **Prefijos jerárquicos:** Si un autor escribe a `"/a/b/c"` y luego a `"/a"`, la segunda escritura borra la primera (y todas las que tengan `"/a"` como prefijo) porque son del mismo autor y la key más corta es prefijo.
- **Sync por NamespaceId:** Dos peers sincronizan el contenido completo del mismo `NamespaceId`, incluyendo entradas de todos los autores.

### `actor.rs` — Actor de operaciones
- **`SyncHandle`** — handle clonable que envía mensajes a un actor thread dedicado
- **`Actor`** — procesa acciones secuencialmente: `ImportAuthor`, `ImportNamespace`, `InsertLocal`, `InsertRemote`, `SyncInitialMessage`, `SyncProcessMessage`, `GetExact`, `GetMany`, etc.
- **`OpenReplicas`** — administra qué réplicas están abiertas, con conteo de handles y estado sync
- Usa un canal `async_channel` con capacidad 1024. Los replies usan `oneshot`.

### `engine.rs` — Motor de sincronización en vivo
- **`Engine`** — coordina el actor de almacenamiento (`SyncHandle`) + actor de sync en vivo (`LiveActor`) + gossip. Métodos: `start_sync`, `leave`, `subscribe`, `handle_connection`, `shutdown`.
- **`LiveEvent`** — eventos expuestos al usuario: `InsertLocal`, `InsertRemote`, `ContentReady`, `PendingContentReady`, `NeighborUp/Down`, `SyncFinished`.
- **`DefaultAuthor`** — author persistente por nodo (memoria o archivo).
- **`ProtectCallbackHandler`** — callback para garbage collection de blobs: protege hashes referenciados en docs.

### `engine/live.rs` — Actor de sync en vivo
Coordina per-documento: ejecuta syncs iniciales con peers, mantiene el gossip swarm, gestiona descargas de contenido, emite eventos.

### `engine/gossip.rs` — Actor de gossip
Maneja el swarm de gossip por documento: se une/sale del swarm, reenvía eventos de entrada a peers, recibe entradas de peers.

### `engine/state.rs` — Estado de sync por documento
Máquina de estados para el ciclo de vida de sync de un documento: `NotSyncing` → `SyncRequested` → `AtRest` ↔ `NewNeighbor`/`SyncAgain`.

### `net.rs` + `net/codec.rs` — Red
- **`connect_and_sync`** — inicia sync saliente (Alice): conecta al peer, ejecuta protocolo.
- **`handle_connection`** — acepta sync entrante (Bob): usa callback de aceptación, ejecuta protocolo.
- **`codec.rs`** — codificación/decodificación del wire protocol para el handshake de sync (namespace negotiation + set reconciliation messages).
- **`ALPN`** = `b"/iroh-sync/1"`
- Tipos de error: `AcceptError`, `ConnectError`, `SyncFinished`, `Timings`.

### `protocol.rs` — Integración con iroh
- **`Docs`** — implementa `ProtocolHandler` de iroh. Contiene `Engine` + `DocsApi`.
- **`Builder`** — construye el protocolo: elige storage (memory/persistent), crea el `Engine`, devuelve `Docs`.

### `api.rs` + `api/` — API RPC (irpc)
- **`DocsApi`** — API de alto nivel: `author_create`, `create`, `import`, `list`, `open`, `drop_doc`, `import_and_subscribe`
- **`Doc`** — handle de un documento: `set_bytes`, `set_hash`, `del`, `get_exact`, `get_many`, `start_sync`, `leave`, `subscribe`, `share`, `import_file`, `export_file`
- Usa `irpc` (protocolo RPC interno) con streaming server-side para listados y queries.

### `ticket.rs` — Tickets
- **`DocTicket`** — serializa capacidad (read/write) + lista de peers. Formato base32 con prefijo `doc`.

### `heads.rs` — Author Heads
- **`AuthorHeads`** — mapa de author → último timestamp conocido. Usado para saber si hay novedades (`has_news_for`) y en el protocolo gossip.

### `metrics.rs` — Métricas
Contadores para: entradas locales/remotas, tamaño de entradas, syncs exitosos/fallidos (accept y connect), ticks de actores.

---

## Capas de la arquitectura

```
┌─────────────────────────────────────────┐
│  API (DocsApi / Doc) — irpc RPC         │
├─────────────────────────────────────────┤
│  Engine — coordina sync + gossip        │
│  ├─ LiveActor — sync en vivo por doc    │
│  ├─ GossipActor — gossip swarm por doc  │
│  └─ SyncHandle — actor thread (storage) │
├─────────────────────────────────────────┤
│  net — red (ALPN /iroh-sync/1)         │
│  ├─ connect_and_sync (Alice)            │
│  └─ handle_connection (Bob)             │
├─────────────────────────────────────────┤
│  ranger — algoritmo set reconciliation │
│  sync — Replica, Entry, SignedEntry     │
├─────────────────────────────────────────┤
│  store — persistencia (redb)            │
│  keys — criptografía ed25519            │
└─────────────────────────────────────────┘
```

## Willow y CRDT

`iroh-docs` **no implementa un CRDT clásico** (como un G-Counter, PN-Counter, OR-Set, etc.), pero el **Willow Protocol** en el que se basa tiene propiedades equivalentes:

### Resolución de conflictos determinista

Las reglas de convergencia están en `src/ranger.rs:564` (método `Store::put`):

1. **Precedencia por prefijo:** Una entrada solo se inserta si es _estrictamente mayor_ que todas las entradas existentes cuya clave es prefijo de la nueva clave.
2. **Limpieza de entradas antiguas:** Al insertar, se eliminan todas las entradas cuya clave tiene como prefijo la clave de la nueva entrada y cuyo valor es menor o igual.
3. **Orden de valores (Willow):** Los valores (`Record`) se ordenan por `timestamp` desc, y a igual timestamp por `hash` desc. Es un orden total determinista.

### ¿Por qué converge sin conflictos?

- **Semántica Last-Writer-Wins (LWW)** basada en timestamp del entry + hash como tiebreaker.
- La operación `put` es **conmutativa** e **idempotente**: si dos peers insertan la misma entrada en distinto orden, el estado final es idéntico.
- **Tombstones:** Las eliminaciones son entradas vacías (`Record` con `Hash::EMPTY` y `len=0`) que actúan como deletion markers con timestamp — no hay operación de "delete" separada.
- **Namespace + Author:** Cada entrada está firmada, por lo que el autor es verificable. No hay vectores de versión: el timestamp universal + hash es suficiente para el orden total.

### Diferencia con un CRDT tradicional

| Aspecto | CRDT clásico | iroh-docs (Willow) |
|---------|-------------|-------------------|
| Merge | Operación `merge` sobre estados | Set reconciliation + reglas Willow en `put` |
| Transporte | Transmite el estado completo o deltas | Descubre diferencias con fingerprints |
| Conflictos | Matemáticamente imposibles por diseño | Resueltos por timestamp + hash (LWW) |
| Vectores de versión | Suele usar version vectors | No usa — el timestamp universal basta |

La **set reconciliation** (ranger) es la capa de transporte que descubre _qué entradas faltan_, y **Willow** es la capa semántica que decide _qué entrada gana_ ante concurrencia.

## ¿Es mejor que un CRDT simple?

**Depende del caso de uso.** iroh-docs usa dos mecanismos en capas que resuelven problemas distintos:

### Willow como capa de convergencia (equivalente funcional a CRDT)

| Propiedad CRDT | Cumplimiento en Willow |
|---|---|
| Convergencia eventual | ✓ — dos réplicas que ven el mismo conjunto de entradas llegan al mismo estado |
| Conmutatividad | ✓ — `put` es conmutativo, el orden de inserción de entradas no altera el resultado final |
| Idempotencia | ✓ — insertar la misma entrada dos veces produce el mismo estado |
| Asociatividad | ✓ — el merge de estados es asociativo |

Willow logra esto **sin vectores de versión** — solo timestamp + hash como orden total.

### Set Reconciliation como capa de transporte (ventaja sobre CRDTs tradicionales)

| | CRDT clásico (state-based) | CRDT clásico (op-based) | iroh-docs (set reconciliation) |
|---|---|---|---|
| Qué se transmite | Estado completo | Log de operaciones | Fingerprints + solo las diferencias |
| Ancho de banda | O(n) — todo el estado | O(k) — k operaciones nuevas | O(m) — m entradas diferentes, con overhead logarítmico |
| Útil si diferencias son | Cualquiera | Pocas ops | Pocas entradas distintas |
| Útil si conjuntos son | Pequeños | Cualquiera | Grandes y con alta superposición |
| Requiere orden causal | No | Sí | No |

**Conclusión:** Es superior a un CRDT state-based para conjuntos grandes con pocas diferencias (el caso común en sync periódica), pero tiene CPU extra por fingerprint y recursión. Para conjuntos chicos o con cambios masivos, un CRDT op-based puede ser más simple.

### Limitaciones

- **No es un CRDT offline-first completo:** El orden total depende del timestamp del dispositivo. Si dos peers escriben "simultáneamente" con relojes desincronizados, el hash decide, no la intención del usuario.
- **No hay merge personalizado por aplicación:** La resolución de conflictos es fija (LWW), no programable.
- **Causalidad débil:** No hay happens-before explícito — si Alice borra y Bob edita concurrentemente, gana el timestamp más alto.

---

## Escalabilidad y eficiencia

### ¿Cuánto puede crecer?

- **Store:** `redb` (B-tree empotrado). Escala hasta el tamaño del archivo en disco. Cada entrada ocupa ~200-500 bytes serializados (32B namespace + 32B author + key + 8B len + 32B hash + 8B timestamp + 128B firmas).
- **Memoria en runtime:** Solo mantiene réplicas abiertas en memoria (`OpenReplicas`). Los datos se leen del store vía iteradores de rango.
- **Costo de sync:** El algoritmo de set reconciliation hace O(n log n) fingerprint en el peor caso dividiendo rangos recursivamente. Con `split_factor=2`, cada ronda divide a la mitad, por lo que la profundidad crece logarítmicamente con el tamaño del conjunto.

### ¿Es eficiente?

| Factor | Comportamiento |
|---|---|
| Fingerprints | BLAKE3 sobre entradas (rápido, ~1GB/s/core). Pero cada iteración de rango recalcula fingerprints. |
| Rondas de sync | O(log n) si los conjuntos son similares. En el peor caso (conjuntos muy distintos), puede enviar todas las entradas. |
| Batching de escrituras | `MAX_COMMIT_DELAY = 500ms`. Varias escrituras se agrupan en una transacción `redb`, reduciendo fsyncs. |
| Actor secuencial | Un solo thread para el store — no hay contención de locks pero tampoco paralelismo de escritura. |
| Firma de entradas remotas | Verificación ed25519 por cada entrada recibida en sync — CPU por entrada. |

> **Regla práctica:** Funciona bien para documentos con miles a cientos de miles de entradas sincronizando periódicamente. Para millones, la latencia de sync puede volverse notable por el costo de rangos y fingerprints recursivos.

### Velocidad de escritura

| Operación | Latencia | Notas |
|---|---|---|
| `insert_local` | < 100µs (memoria) / ~1ms (disco) | Se encola en el actor, se procesa secuencialmente |
| `insert_remote` | Igual + verificación ed25519 (~50µs) | La firma se verifica en validación |
| Flush a disco | Cada 500ms máximo | Las escrituras se acumulan en la transacción actual |
| Latencia percibida por el usuario | Hasta 500ms para confirmación durable | Por el batching; usar `flush_store()` para forzar commit inmediato |

La escritura en sí es rápida (insert en B-tree + notificar subscribers). El cuello de botella es el commit periódico a disco. Para writes de alta frecuencia, el batching ayuda a throughput sacrificando latencia de confirmación.

## Estrategias de deployment

| | Tauri (desktop + mobile) | Browser WASM |
|---|---|---|
| **Persistencia** | `redb` con `fs-store` en disco real | `redb` con `InMemoryBackend` (se pierde al cerrar) |
| **Networking** | QUIC/UDP directo + hole punching | Todo vía relay (no P2P directo) |
| **Engine completo** | ✅ Sync + gossip + blobs | ✅ Networking vía relay, limitado |
| **Multi-tenant** | Un `docs.redb` por tenant en disco | Un `Store::memory()` por tenant, volátil |
| **iOS / Android** | ✅ Tauri mobile | ⚠️ Solo browsers con OPFS limitado |
| **Estado actual** | ✅ Listo para producción | ❌ Sin backend OPFS para persistencia |

### Para web app completa (sin Tauri)

Hoy solo funciona con `Store::memory()` — los datos no sobreviven un refresh. Hasta que exista un backend `redb` para OPFS (técnicamente factible, no implementado), una web app con persistencia real necesita un servidor Rust que corra el `Engine` con `fs-store` y exponga la API vía `irpc/noq` al frontend WASM.

### Tauri: cobertura de plataformas

| Plataforma | Tauri v2 | iroh |
|---|---|---|
| Windows (x86_64) | ✅ | ✅ |
| Linux (x86_64, aarch64) | ✅ | ✅ |
| macOS (x86_64, M-series) | ✅ | ✅ |
| iOS | ✅ | ✅ |
| Android | ✅ | ✅ |

Ambos coinciden — donde iroh tiene soporte nativo completo (QUIC directo, hole punching, fs-store), Tauri provee el empaquetado de app con UI web + backend Rust nativo.

### Tauri: arquitectura típica

```
┌─ Tauri App ───────────────────────────────┐
│                                            │
│  Frontend (HTML/CSS/JS)                    │
│  ├─ UI de documentos                       │
│  └─ invoke("set_bytes", {key, val})        │
│         │                                  │
│         ▼  (Tauri commands)                │
│  Backend (Rust nativo)                     │
│  ├─ Docs::persistent("data/")  ◄─── redb   │
│  │   └─ Engine::spawn()                   │
│  ├─ Endpoint::bind()           ◄─── QUIC   │
│  └─ Sync con otros peers                  │
│                                            │
│  data/docs.redb  ← archivo real            │
│  data/default-author                       │
└────────────────────────────────────────────┘
```

---

## Modelo de seguridad y permisos

El modelo de `iroh-docs` es **capability-based security** — no hay ACL ni servidor de autorización.

### El secreto ES el permiso

| Acceso | Requiere |
|---|---|
| **Escribir** en un doc | `NamespaceSecret` (clave privada ed25519 de 32 bytes) |
| **Leer** un doc | `NamespaceId` (clave pública, derivada del secreto) |

No hay forma de autorizar a otro peer sin darle la clave. El permiso está en la posesión del secreto.

### Cómo se comparte el acceso

```rust
// Peer A (dueño) comparte acceso de escritura con Peer B
let ticket: DocTicket = doc.share(ShareMode::Write, addr_options).await?;
// ticket.to_string() → "doc..." (string base32, copiable/escaneable)

// Peer B recibe el ticket (fuera de banda: QR, link, copiar/pegar)
let (doc, events) = api.import_and_subscribe(ticket).await?;
// Peer B ya puede escribir en el documento
```

El `DocTicket` (`src/ticket.rs`) encapsula:
- `Capability` — `Write(NamespaceSecret)` o `Read(NamespaceId)`
- `nodes: Vec<EndpointAddr>` — peers iniciales para conectar

### Limitaciones del modelo de permisos

| Propiedad | Comportamiento |
|---|---|
| **Revocación** | ❌ No existe — quien tiene el secreto puede firmar para siempre |
| **Escritores múltiples** | ✅ Cualquiera con el secreto puede escribir |
| **Saber quién escribió** | ✅ Cada entrada firmada por `Author`, verificable |
| **Rotación de clave** | Creando un namespace nuevo; los tickets viejos no sirven |
| **Permisos por key** | ❌ No — el permiso es todo o nada a nivel namespace |
| **Permisos por autor** | ❌ No — cualquier autor con el secreto del namespace puede escribir |

### Quién escribió qué

Aunque varios peers comparten el `NamespaceSecret`, cada entrada se firma con el `Author` individual:
```
Entry firmada por: [namespace_sig compartido] + [author_sig del peer específico]
```

Esto permite verificar qué peer escribió cada key, pero no impide que otro peer con el secreto escriba en keys ajenas.

---

## Multi-tenant

El `Endpoint` de iroh puede compartirse entre múltiples instancias de `Docs` independientes. La estrategia recomendada es **un `docs.redb` por tenant**:

```
data/
├── tenant_acme/
│   └── docs.redb          ← solo namespaces de Acme
├── tenant_beta/
│   └── docs.redb          ← solo namespaces de Beta
└── tenant_gamma/
    └── docs.redb          ← solo namespaces de Gamma
```

```rust
let endpoint = Endpoint::bind(presets::N0).await?;
let blobs = MemStore::default();
let gossip = Gossip::builder().spawn(endpoint.clone());

let docs_acme = Docs::persistent("data/tenant_acme".into())
    .spawn(endpoint.clone(), blobs.clone(), gossip.clone()).await?;

let docs_beta = Docs::persistent("data/tenant_beta".into())
    .spawn(endpoint.clone(), blobs.clone(), gossip.clone()).await?;
```

**Costo por tenant:** ~1 thread (actor) + 1 conexión redb. Para cientos de tenants, evaluar si el overhead de threads es aceptable.

**Aislamiento:** Los tenants no pueden acceder a namespaces de otro — están en archivos físicamente separados.

---

## Casos de uso: qué SÍ y qué NO

### Para lo que SÍ está diseñado

- ✅ Documentos colaborativos con múltiples escritores
- ✅ Sincronización eventual entre pares P2P
- ✅ Key-value con jerarquía de prefijos (tipo filesystem)
- ✅ Offline-first con resolución automática de conflictos
- ✅ Aplicaciones donde el último valor es el que importa (LWW)

### Para lo que NO está diseñado

- ❌ **Append logs** — Willow permite sobrescribir entradas (LWW), no acumular. Forzar keys secuenciales (`/log/0001`, `/log/0002`) técnicamente funciona pero pagás el costo de firmas ed25519 por entrada y set reconciliation sin beneficiarte de Willow.
- ❌ **Bases de datos relacionales** — no hay joins, transacciones multi-key, ni queries complejos.
- ❌ **Archivos grandes** — los datos reales van en `iroh-blobs`, no en la réplica.
- ❌ **Permisos revocables o granulares** — el modelo de capability es todo o nada por namespace.
- ❌ **Orden causal estricto** — no hay happens-before explícito; dos escrituras concurrentes las resuelve el timestamp + hash.

Referencias:
- [Willow Protocol Specification](https://hackmd.io/DTtck8QOQm6tZaQBBtTf7w) (referenciado en `sync.rs:5`)
- [Range-Based Set Reconciliation (Meyer)](https://arxiv.org/abs/2212.13567)
- `src/actor.rs:36` — `MAX_COMMIT_DELAY = 500ms`
- `src/ranger.rs:673-687` — `SyncConfig` con `max_set_size=1, split_factor=2`

---

## Flujo de sincronización

### Protocolo: solo se transmite lo que falta

El algoritmo en `ranger.rs:process_message` (línea 324) garantiza que **solo se intercambian las entradas diferentes** entre dos peers.

```
Alice (inicia)                            Bob (responde)
─────────────                             ──────────────
1. fingerprint(all) ──────────────────→   
                                         2. ¿mi_fp == fp_remoto?
                                            ├─ SÍ → fin (rangos idénticos)
                                            └─ NO  → ¿tamaño ≤ 1?
                                                      ├─ SÍ → envío mis entradas
                                                      └─ NO  → parto en 2 subrangos
                                                                envío fp(subrango1) + fp(subrango2)
                                          ←───────────────────
3. proceso cada parte:
   ¿fp coincide?
   ├─ SÍ → skip
   └─ NO  → misma lógica (ancla o split)
                                          ...continúa hasta converger...
```

- **Fingerprint vacío** (`Fingerprint::empty()`) significa "el otro peer no tiene entradas en este rango" → se envía todo el rango.
- **Cada ronda descarta los subrangos idénticos** y solo profundiza en los que difieren.
- **Si las diferencias son pocas**, el número de mensajes es bajo (los fingerprints coinciden rápido y se envían solo los entries distintos).
- **En el peor caso** (cero entradas en común), se transmiten todas las entradas.

### `have_local` — Evitar doble envío

Cuando Bob envía items a Alice, ella calcula `diff`: de sus entradas locales en ese rango, **descarta las que Bob ya tiene** con valor igual o mayor (`ranger.rs:360-382`). Solo envía de vuelta lo que Bob no tiene o lo que Alice tiene más nuevo.

### Paso a paso completo

1. **Apertura:** `SyncHandle::open` carga la réplica del store
2. **Escritura local:** `Replica::insert` → firma entry → `ranger::Store::put` → emite `Event::LocalInsert`
3. **Sync inicial:** `Engine::start_sync` → `LiveActor` inicia `connect_and_sync` con cada peer
4. **Protocolo:** Alice envía fingerprint inicial, Bob responde con fingerprint o items, se repite recursivamente hasta converger
5. **Entradas remotas:** `Replica::insert_remote_entry` → valida firma → `put` en store → emite `Event::RemoteInsert`
6. **Gossip:** Una vez sincronizado, el documento se une al swarm gossip para recibir actualizaciones en tiempo real
7. **Descarga de contenido:** El engine consulta `ContentStatusCallback` (basado en `iroh_blobs`) para determinar si el blob asociado al hash de cada entry está disponible localmente
