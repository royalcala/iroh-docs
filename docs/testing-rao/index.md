# iroh-docs — Análisis de Arquitectura

**Repositorio:** `n0-computer/iroh-docs` — Rust crate para documentos key-value multidimensionales con sincronización eficiente.

**Documentos relacionados:**
- [Mapeo de conceptos](mappings.md) — SQL, red, CRDT, claves, API, glosario
- [API de lectura de iroh-docs](../liveStore/iroh-docs-reading-api.md)
- [Decisión de arquitectura](decision.md) — Arquitectura final simple
- [syntrix-docs: wrapper de autorización](syntrix-docs.md) — Cómo funcionan roles y permisos
- [Síntesis de modelos de IA](synthesis.md)
- [Prompt para otros modelos](erp-prompt.md)

## Resumen

`iroh-docs` es un sistema de sincronización de documentos basado en **replicas**. Cada réplica es un espacio de nombres (namespace) que contiene entradas key-value firmadas por dos pares de claves ed25519: el **Author** (autoría) y el **Namespace** (permiso de escritura). Los datos reales (blobs) no se almacenan en la réplica — solo su hash BLAKE3, tamaño y timestamp.

La sincronización entre pares usa **set reconciliation basado en rangos** (paper de Aljoscha Meyer) con fingerprints BLAKE3 recursivos. El modelo de datos y resolución de conflictos sigue **Willow Protocol** (propiedades CRDT-like, convergencia determinista LWW).

## Capas de la arquitectura

```
┌─ API (DocsApi / Doc) — irpc RPC ─────────────┐
├─ Engine — coordina sync + gossip               │
│  ├─ LiveActor — sync en vivo por doc           │
│  ├─ GossipActor — gossip swarm por doc         │
│  └─ SyncHandle — actor thread (storage)        │
├─ net — ALPN /iroh-sync/1 (QUIC)               │
│  ├─ connect_and_sync (Alice)                   │
│  └─ handle_connection (Bob)                    │
├─ ranger — set reconciliation algorithm         │
├─ sync — Replica, Entry, SignedEntry            │
├─ store — persistencia redb (B-tree)            │
└─ keys — criptografía ed25519                   │
```

## Modelo de datos

```
Entry = RecordIdentifier [ns:32B | author:32B | key:bytes] + Record [hash:32B | len:8B | ts:8B]
SignedEntry = Entry + EntrySignature [ns_sig:64B | author_sig:64B]

UN solo archivo docs.redb contiene TODOS los namespaces.
Internamente redb usa NamespaceId como prefijo de 32B en la key del B-tree.
```

## Willow y CRDT

Willow usa LWW determinista: timestamp (μs) + hash como tiebreaker. Operación `put` conmutativa, idempotente, asociativa. Sin vectores de versión. Tombstones = entries con `Hash::EMPTY` y `len=0`.

## Sync: solo se transmite lo que falta

Protocolo de set reconciliation: Alice envía fingerprint, Bob compara. Si coinciden = skip. Si difieren y el rango es grande = split recursivo. Solo se envían entries cuando el rango es ≤1 elemento o fingerprint vacío. El campo `have_local` evita reenviar datos que el otro peer ya tiene.

## Estrategias de deployment

| | Tauri (desktop + mobile) | Browser WASM |
|---|---|---|
| **Persistencia** | `redb` con `fs-store` en disco real | `redb` con `InMemoryBackend` (volátil) |
| **Networking** | QUIC/UDP directo + hole punching | Todo vía relay |
| **Multi-tenant** | Un `docs.redb` por tenant en disco | Un `Store::memory()` por tenant |
| **iOS / Android** | ✅ Tauri mobile | ❌ Sin backend OPFS |

## Seguridad y permisos

- **Capability-based:** `NamespaceSecret` = Write, `NamespaceId` = Read
- **Sin revocación:** quien tuvo el secreto, lo tiene para siempre
- **Sin permisos por key/author:** binario por namespace
- **Tickets:** string base32 que contiene Capability + lista de peers. Se comparte fuera de banda (QR, link, texto)
- **Author signatures:** auditan quién escribió, no previenen

## PRs al upstream

| PR | Estado | Rama |
|---|---|---|
| `key_prefix_from` cursor en Query | [#108](https://github.com/n0-computer/iroh-docs/pull/108) abierto | `feat/query-key-prefix-from` |
| Issue relacionado | [#109](https://github.com/n0-computer/iroh-docs/issues/109) | — |

## Referencias

- [Willow Protocol](https://hackmd.io/DTtck8QOQm6tZaQBBtTf7w) — `sync.rs:5`
- [Range-Based Set Reconciliation](https://arxiv.org/abs/2212.13567)
- `src/actor.rs:36` — `MAX_COMMIT_DELAY = 500ms`
- `src/ranger.rs:673-687` — `SyncConfig { max_set_size: 1, split_factor: 2 }`
