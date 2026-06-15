# iroh-docs — Mapeo de conceptos

## Desde cero: cómo dos nodos sincronizan

### 1. Cada nodo es una identidad

Cuando un nodo inicia, genera su propia clave secreta:

```
Nodo A                           Nodo B
let endpoint = Endpoint::bind()  let endpoint = Endpoint::bind()
  → SecretKey = random 32B         → SecretKey = random 32B
  → EndpointId = 0xAAA...          → EndpointId = 0xBBB...
```

El `EndpointId` es como su "nombre en la red" — 32 bytes derivados de la clave. No es una IP. Con eso, otros nodos lo encuentran vía relay o DHT.

### 2. Cada nodo crea su propio Author

Un `Author` es una identidad para firmar entradas dentro de un doc:

```
Nodo A                           Nodo B
let author_a = docs.author_create()  let author_b = docs.author_create()
  → Author = random 32B secret       → Author = random 32B secret
  → AuthorId = 0xALICE               → AuthorId = 0xBOB
```

El `Author` es **local** — no se comparte. Cada nodo tiene el suyo.

### 3. Un nodo crea el doc (Namespace)

Un doc es un `Namespace` — un espacio de keys compartido:

```
Nodo A
let doc = docs.create().await?
  → NamespaceSecret = random 32B   ← ESTO es el "permiso de escritura"
  → NamespaceId = 0xDEF...         ← ESTO es el "nombre del doc" (público)
```

En ESTE momento, solo el Nodo A conoce el `NamespaceSecret`. Solo él puede escribir.

### 4. El Nodo A escribe datos

```
doc.set_bytes(author_a, "evt:001", b"hola").await?
doc.set_bytes(author_a, "evt:002", b"mundo").await?
```

Cada entry se guarda en SU `docs.redb`:

```
Nodo A — docs.redb
├── ns: 0xDEF...
│   ├── Entry [0xDEF | 0xALICE | "evt:001"] → hash=0xH1, ts=...
│   └── Entry [0xDEF | 0xALICE | "evt:002"] → hash=0xH2, ts=...
```

### 5. Nodo A genera un ticket para compartir

El ticket es un "link de invitación" que contiene el secreto:

```
Nodo A
let ticket = doc.share(ShareMode::Write).await?
→ ticket.to_string() = "doc..."
```

Dentro del ticket viajan:
- `NamespaceSecret` (0xDEF...) ← el secreto para escribir
- `EndpointAddr` de A ← cómo contactar al Nodo A

Nodo A le pasa ese string al Nodo B (QR, link, texto).

### 6. Nodo B importa el ticket

```
Nodo B
let ticket: DocTicket = "doc...".parse()?
let (doc, events) = docs.import_and_subscribe(ticket).await?
```

Esto hace 3 cosas:
1. **Guarda el secreto** en SU `docs.redb` → ahora B también puede escribir
2. **Contacta al Nodo A** usando la dirección del ticket
3. **Inicia la sincronización** → recibe `"evt:001"` y `"evt:002"`

```
Nodo B — docs.redb (después de la sync)
├── ns: 0xDEF...
│   ├── Entry [0xDEF | 0xALICE | "evt:001"] → hash=0xH1, ts=...  ← de A
│   └── Entry [0xDEF | 0xALICE | "evt:002"] → hash=0xH2, ts=...  ← de A
```

### 7. Ahora ambos escriben — gossip en vivo

```
Nodo A                             Nodo B
doc.set_bytes(author_a,            doc.set_bytes(author_b,
  "evt:003", data)                   "evt:004", data)
      │                                   │
      └── gossip ──────────────────────►  │  B recibe en vivo
      │                                   │
      │  A recibe en vivo ◄───────────────┘  gossip
      ▼                                   ▼
```

Ambos terminan con **la misma data**, cada uno en su propio `docs.redb`:

```
AMBOS nodos — cada uno en su docs.redb
├── ns: 0xDEF...
│   ├── [0xDEF | 0xALICE | "evt:001"]  ← escribió A
│   ├── [0xDEF | 0xALICE | "evt:002"]  ← escribió A
│   ├── [0xDEF | 0xALICE | "evt:003"]  ← escribió A
│   └── [0xDEF | 0xBOB   | "evt:004"]  ← escribió B
```

### Resumen

| Pregunta | Respuesta |
|---|---|
| ¿Hay una DB central? | No. Cada nodo tiene su propio `docs.redb`. |
| ¿Qué comparten? | El `NamespaceSecret` (vía ticket). Con eso sincronizan entries. |
| ¿Qué NO comparten? | Los `Author` (cada nodo tiene el suyo). Las claves privadas del Endpoint. |
| ¿Cómo saben qué entries intercambiar? | Set reconciliation: comparan fingerprints, solo mandan diferencias. |
| ¿Qué pasa si ambos escriben la misma key? | Gana el timestamp más alto (LWW). Si mismo timestamp, gana hash mayor. |
| ¿Qué identifica a un nodo? | `EndpointId` (32 bytes, derivado de su clave secreta). |
| ¿Qué identifica a un escritor? | `AuthorId` (32 bytes, derivado de su Author). |
| ¿Qué identifica al doc? | `NamespaceId` (32 bytes, derivado del NamespaceSecret). |
| ¿El ticket expira? | No. Quien tiene el ticket tiene acceso para siempre. |

---

## Base de datos relacional → iroh-docs

## Base de datos relacional → iroh-docs

```
SQL                                         iroh-docs
────────────────────────────                ────────────────────────────────
Database      CREATE DATABASE mydb;         Store (docs.redb — UN archivo)
Schema        (implícito)                   No hay schema fijo
Table         CREATE TABLE users (...);     NamespaceId (32 bytes)
               └── columnas fijas            └── key-value sin columnas
Row           INSERT INTO users ...         Entry / SignedEntry
PRIMARY KEY   (id INTEGER)                  RecordIdentifier [ns|author|key]
               └── autoincrement             └── tu key son los bytes que quieras
Columna       name TEXT                     Parte de los bytes de la key por convención
                                            (ej: "users/alice/name")
Valor         'Alice'                       Record { hash, len, timestamp }
                                             └── los datos reales van en iroh-blobs
DELETE        DELETE FROM users WHERE id=1  Entry vacío (tombstone, hash=EMPTY)
WHERE         WHERE key LIKE 'evt:%'        Query::key_prefix("evt:")
ORDER BY      ORDER BY key ASC              B-tree ordena lexicográficamente
LIMIT/OFFSET  LIMIT 10 OFFSET 5             Query::limit(10).offset(5)
                                             └── ⚠️ offset es client-side
JOIN          (no existe)                   ❌ No hay joins
Transactions  BEGIN ... COMMIT              ❌ No hay transacciones multi-key
```

### Ejemplo práctico

```sql
-- SQL
CREATE TABLE events (
    id TEXT PRIMARY KEY,    -- "evt:<HLC>:<node_id>"
    payload_hash BLOB,
    payload_len INTEGER,
    timestamp INTEGER
);
INSERT INTO events VALUES ('evt:01J...:node_a', X'...', 42, 1718400000);
SELECT * FROM events WHERE id LIKE 'evt:%' ORDER BY id;

-- iroh-docs
let doc = api.create().await?;
doc.set_hash(author, "evt:01J...:node_a", hash, 42).await?;
let stream = doc.get_many(Query::all().key_prefix("evt:")).await?;
```

---

## Identidad y red → iroh + iroh-docs

```
Concepto de red                             iroh / iroh-docs
────────────────────────────                ────────────────────────────────
IP:puerto        192.168.1.1:8080          ❌ No se usa
Identidad nodo   (no existe en TCP)        EndpointId = PublicKey (32 bytes)
                                             └── PeerIdBytes en iroh-docs
Dirección        socket addr               EndpointAddr { node_id, relay_url, direct_addrs }
Conexión         TCP connection            QUIC connection (encriptada E2E)
Protocolo        HTTP, gRPC                ALPN = b"/iroh-sync/1"
Puerto           :443, :8080               ❌ No necesario (hole punching)
DNS              nombre → IP               Pkarr / DHT: EndpointId → direcciones
Relay/TURN       centralizado              Relay servers iroh (públicos o self-hosted)
```

### Jerarquía de identidad

```
Endpoint (el nodo)
 └── EndpointId  =  PublicKey  =  [u8; 32]  ← identidad del nodo en la red
      └── se deriva de SecretKey (generado al iniciar)

PeerIdBytes  =  [u8; 32]  ← alias de EndpointId en iroh-docs
                             (identidad del peer remoto que envió una entry)
```

---

## CRDT / Sincronización → iroh-docs

```
Concepto distribuido                        iroh-docs
────────────────────────────                ────────────────────────────────
Réplica         副本                       Namespace / Doc (NamespaceId)
Estado          状态                       Conjunto de SignedEntry en el namespace
Merge           merge(state_a, state_b)    Set reconciliation (ranger.rs)
                                               + Willow rules (put)
Conflicto       conflicto                  ❌ No hay — LWW por timestamp + hash
Resolución      last-writer-wins           timestamp (μs), tiebreaker: hash
Vector clock    [A:3, B:1, C:5]           ❌ No se usa — timestamp universal
Tombstone       deletion marker            Entry con Hash::EMPTY y len=0
CRDT type       G-Counter, OR-Set         Willow Protocol (LWW sobre keys con prefijos)
Offline-first   escribe sin red            ✅ Sí — sync al reconectar
Causalidad      happens-before             ❌ No — solo timestamp + hash
```

---

## Claves y firmas → iroh-docs

```
Tipo                Tamaño    Rol
────────────────    ──────    ──────────────────────────────────
NamespaceSecret     32 bytes  Clave privada del namespace
                               └── quien la tiene PUEDE escribir
NamespaceId         32 bytes  Clave pública del namespace
                               └── identifica el doc, permite leer
NamespacePublicKey  32 bytes  Clave pública (verifica firmas)
Author              32 bytes  Clave privada del autor
                               └── identidad de quien escribe
AuthorId            32 bytes  Clave pública del autor
AuthorPublicKey     32 bytes  Clave pública (verifica firmas)
PeerIdBytes         32 bytes  EndpointId del peer remoto
                               └── quién envió la entry en sync
```

### Relaciones

```
NamespaceSecret ──deriva──► NamespaceId ──es──► identificador del Doc
        │                      │
        │ firma                │ verifica
        ▼                      ▼
   EntrySignature        EntrySignature
   .namespace_signature   .namespace_signature

Author ──deriva──► AuthorId ──es──► identificador del escritor
   │                    │
   │ firma              │ verifica
   ▼                    ▼
EntrySignature        EntrySignature
.author_signature      .author_signature
```

---

## Estructura de datos

```
Entry {
    id: RecordIdentifier {
        namespace: [u8; 32],   // NamespaceId — ¿en qué doc?
        author:    [u8; 32],   // AuthorId    — ¿quién escribe?
        key:       [u8],       // bytes       — ¿qué key?
    },
    record: Record {
        len:       u64,        // tamaño del contenido real (en iroh-blobs)
        hash:      [u8; 32],   // BLAKE3 del contenido real
        timestamp: u64,        // μs desde UNIX epoch
    }
}

SignedEntry {
    signature: EntrySignature {
        namespace_signature: [u8; 64],  // ed25519(namespace_secret, entry_bytes)
        author_signature:    [u8; 64],  // ed25519(author_secret, entry_bytes)
    },
    entry: Entry
}
```

### Tamaños en disco

| Componente | Bytes |
|---|---|
| RecordIdentifier (namespace + author) | 64 fijos |
| Key | N (variable) |
| Record (len + hash + timestamp) | 8 + 32 + 8 = 48 |
| EntrySignature (2 × ed25519) | 64 + 64 = 128 |
| **Total por entry (sin key)** | **~240 bytes** |
| **Total con key de ~50 bytes** | **~300 bytes** |

---

## API: operaciones principales

```
Operación                   API                             ¿Qué hace en el store?
────────────────────────    ───────────────────────────    ──────────────────────
Crear doc                   api.create()                   Genera NamespaceSecret + NamespaceId
Importar doc (ticket)       api.import(ticket)             Guarda Capability en NAMESPACES_TABLE
Escribir (datos)            doc.set_bytes(author, k, v)    Hashea v → guarda en blobs → inserta entry
Escribir (hash existente)   doc.set_hash(author, k, h, n)  Inserta entry con hash ya conocido
Leer exacto                 doc.get_exact(author, key)     Lookup [ns|author|key] en RECORDS_TABLE
Leer por prefijo            doc.get_many(key_prefix("a/")) Escaneo de rango en B-tree
Leer por autor              doc.get_many(author(alice))    Escaneo de rango [ns|alice|...]
Borrar (prefijo)            doc.del(author, "a/")          Inserta entry vacía con key="a/"
Sync inicio                 doc.start_sync(peers)          Engine inicia LiveActor + gossip
Sync stop                   doc.leave()                    Sale del swarm gossip
Subscribir eventos          doc.subscribe()                Recibe InsertLocal, InsertRemote, SyncFinished
Compartir (ticket)          doc.share(mode)                Serializa Capability + peers → DocTicket
Listar docs                 api.list()                     Iterador sobre NAMESPACES_TABLE
Eliminar doc                api.drop_doc(id)               Borra namespace + todas sus entries
```

---

## Layout en disco (redb)

```
docs.redb
│
├── Table: "namespaces-1"
│   Key:   [NamespaceId: 32B]
│   Value: (capability_kind: u8, raw_bytes: [u8; 32])
│
├── Table: "records-1"
│   Key:   (NamespaceId: 32B, AuthorId: 32B, key: bytes)
│   Value: (timestamp: u64, ns_sig: [u8; 64], author_sig: [u8; 64], len: u64, hash: [u8; 32])
│   Orden: lexicográfico → todos los entries de un NS son contiguos
│
├── Table: "records_by_key-1" (índice secundario)
│   Key:   (NamespaceId: 32B, key: bytes, AuthorId: 32B)
│   Value: (timestamp: u64, ns_sig: [u8; 64], author_sig: [u8; 64], len: u64, hash: [u8; 32])
│   Orden: por key → útil para prefijos y key_exact
│
├── Table: "latest_per_author-1"
│   Key:   (NamespaceId: 32B, AuthorId: 32B)
│   Value: (timestamp: u64, key: bytes)
│
├── Table: "namespace_peers-1"
│   Key:   [NamespaceId: 32B]
│   Value: (last_seen: u64, peer_id: [u8; 32])
│
└── Table: "download_policy-1"
    Key:   [NamespaceId: 32B]
    Value: policy (serialized)
```

---

## Flujo de una sincronización completa

```
Paso  Actor          Acción
────  ─────          ──────
  1   Doc::start_sync(peers)
  2   LiveActor      Por cada peer: connect_and_sync()
  3   Alice          Envía fingerprint(rango_completo)
  4   Bob            Compara fingerprint local vs remoto:
                     ├─ Coinciden → fin (rangos idénticos)
                     └─ Difieren  → divide rango en 2, envía 2 fingerprints
  5   Alice          Recibe fingerprints, repite lógica:
                     ├─ Coinciden → skip
                     ├─ Difieren + ≤1 entry → envía entry
                     └─ Difieren + >1 entry → divide de nuevo
  ... se repite hasta converger ...
  N   Ambos          Tienen el mismo conjunto de entries
  N+1 LiveActor      Se une al gossip swarm (topic = NamespaceId)
  N+2 GossipActor    Recibe/broadcast nuevos entries en tiempo real
  N+3 Downloader     Descarga blobs para entries remotas (ContentReady)
```

---

## Glosario rápido

| Término | Significado |
|---|---|
| **Namespace** | Un documento, un espacio de keys sincronizable. |
| **NamespaceId** | Identificador público del namespace (32 bytes). |
| **NamespaceSecret** | Clave privada del namespace — quien la tiene puede escribir. |
| **Author** | Identidad de un escritor (par de claves ed25519). |
| **AuthorId** | Identificador público del autor. |
| **Entry** | Un registro: key + hash + len + timestamp. |
| **SignedEntry** | Entry firmada por namespace + author. |
| **Record** | El valor de una entry: (hash, len, timestamp). |
| **RecordIdentifier** | Clave compuesta: (namespace, author, key). |
| **Capability** | Permiso: Write (tiene la clave) o Read (solo ID público). |
| **DocTicket** | Ticket serializado: Capability + lista de peers. |
| **EndpointId** | Identidad de un nodo en la red iroh (clave pública). |
| **PeerIdBytes** | Alias de EndpointId en iroh-docs ([u8; 32]). |
| **Set Reconciliation** | Algoritmo que descubre diferencias entre dos conjuntos con fingerprints. |
| **Willow** | Protocolo que define cómo resolver conflictos (LWW con prefijos). |
| **Gossip** | Protocolo de difusión de novedades en tiempo real entre peers. |
| **Blobs** | Almacenamiento de los datos reales (hash-addressable). |
| **redb** | Base de datos embebida (B-tree) que persiste las entries. |
