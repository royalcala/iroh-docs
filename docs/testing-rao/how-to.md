# Guía de inicio: Syntrix ERP

## Stack completo

| Capa | Tecnología | Rol |
|---|---|---|
| Shell desktop | Tauri v2 | Empaqueta la app para Windows, macOS, Linux, iOS, Android |
| P2P networking | iroh (QUIC, hole punching, relay) | Conexiones directas entre peers sin servidor |
| Storage + sync | iroh-docs | Key-value sincronizable con set reconciliation |
| Autorización | iroh-syntrix-docs | NamespaceRegistry + accept_cb, lee control namespace |
| Base de datos local | LiveStore → SQLite | Materializa namespaces en tablas relacionales |
| Frontend | Vite + React 19 + TanStack Router | UI de la app |
| Monorepo | Turborepo | Orquesta builds Rust + TypeScript |

## Estructura del proyecto (monorepo con Turborepo)

```
syntrix/
├── turbo.json                    # pipeline de builds
├── package.json                  # workspace root
├── pnpm-workspace.yaml
│
├── apps/
│   └── desktop/                  # Tauri app
│       ├── src-tauri/            # Rust backend
│       │   ├── Cargo.toml
│       │   ├── tauri.conf.json
│       │   └── src/
│       │       ├── main.rs       # entry point
│       │       ├── commands.rs   # #[tauri::command] handlers
│       │       └── syntrix.rs    # inicia iroh + iroh-syntrix-docs + LiveStore
│       ├── src/                  # React frontend
│       │   ├── main.tsx
│       │   ├── router.tsx        # TanStack Router
│       │   ├── routes/
│       │   └── components/
│       ├── index.html
│       ├── vite.config.ts
│       └── package.json
│
├── packages/
│   ├── livestore/               # LiveStore (SQLite materialization)
│   │   ├── Cargo.toml           # si es Rust
│   │   └── src/
│   │
│   ├── syntrix-types/           # Tipos compartidos TypeScript
│   │   ├── package.json
│   │   └── src/
│   │       ├── events.ts        # tipos de eventos (invoiceCreated, etc)
│   │       ├── roles.ts         # Role, Member, OrgId
│   │       └── namespaces.ts    # mapeo de namespaces
│   │
│   └── syntrix-ui/              # Componentes React compartidos
│       ├── package.json
│       └── src/
│
├── crates/                      # Crates Rust (no Tauri)
│   ├── iroh-docs/               # tu fork (git submodule o [patch])
│   └── iroh-syntrix-docs/       # wrapper de autorización
│       ├── Cargo.toml
│       └── src/
│
└── docs/
    └── architecture/            # decisión, mapeo, análisis
```

## Dependencias entre capas

```
React (TanStack Router)
  │  invoke("create_invoice", { ... })
  ▼
Tauri commands (commands.rs)
  │  llama a syntrix.rs
  ▼
syntrix.rs (orquestador)
  ├── iroh (Endpoint)
  ├── iroh-docs (Engine)
  │     └── iroh-syntrix-docs (registry + accept_cb)
  └── LiveStore (materializa namespaces → SQLite)
        │
        ▼
      SQLite (tablas relacionales)
```

## Orden de inicialización (arranque de la app)

1. **Tauri** inicia el backend Rust
2. **iroh** crea el `Endpoint` (identidad P2P del dispositivo)
3. **iroh-docs** abre el `Engine` con `docs.redb` persistente
4. **iroh-syntrix-docs** sincroniza los `org_control` namespaces conocidos
5. **iroh-syntrix-docs** determina qué namespaces abrir según rol y active status
6. **LiveStore** abre los namespaces autorizados y materializa en SQLite
7. **React** monta la UI, consume datos de LiveStore vía Tauri commands

## Flujo de escritura (ej: crear invoice)

```
1. Usuario llena formulario en React
2. React → invoke("create_invoice", { client_id, items, total })
3. Tauri command → syntrix.create_invoice(author, data)
4. syntrix → LiveStore valida reglas de negocio
5. LiveStore → iroh-docs.set_bytes(author, "invoice/001", payload)
6. iroh-docs → firma entry con Author + NamespaceSecret
7. iroh-docs → emite Event::LocalInsert
8. iroh-docs → gossip propaga a otros peers de la org
9. LiveStore → actualiza SQLite local
10. React → recibe update vía suscripción
```

## Flujo de sync (peer recibe datos)

```
1. Peer remoto escribe una entry en su namespace
2. iroh-docs → gossip propaga el cambio
3. Nuestro peer recibe el mensaje de gossip
4. accept_cb → ¿peer activo? ¿namespace autorizado? → Allow
5. iroh-docs → insert_remote_entry en nuestro docs.redb
6. LiveStore → detecta nueva entry → actualiza SQLite
7. React → recibe update vía suscripción
```

## ¿Turborepo o no?

**Sí, Turborepo** si tenés al menos 2 de estos:
- Múltiples apps (desktop + mobile + web)
- Paquetes TypeScript compartidos (syntrix-types, syntrix-ui)
- Builds que dependen unos de otros (types → ui → app)

**No, solo Cargo workspace** si:
- Solo una app Tauri
- Sin paquetes TypeScript compartidos
- El frontend vive dentro de `apps/desktop/src/` y ya

Para empezar: **Tauri simple + Cargo workspace para los crates Rust.** Agregás Turborepo solo cuando el frontend crezca y necesites shared packages.

## Qué crear primero (orden sugerido)

| Paso | Qué | Tiempo estimado |
|---|---|---|
| 1 | `pnpm create tauri-app` — scaffold inicial | 5 min |
| 2 | Agregar `iroh`, `iroh-docs` a `Cargo.toml` | 2 min |
| 3 | Crear `crates/iroh-syntrix-docs` (mover lo del branch) | 10 min |
| 4 | `syntrix.rs` — inicializar iroh + Engine + syntrix-docs | 1-2 hrs |
| 5 | `commands.rs` — exponer operaciones básicas (list_namespaces, get_events) | 2-3 hrs |
| 6 | React — router + página básica con lista de eventos | 2-3 hrs |
| 7 | LiveStore — materializar un namespace en SQLite | 3-4 hrs |
| 8 | Flujo completo: crear invoice → sync → ver en otro peer | 1 día |

## Lo que NO necesitás en la v1

- ❌ Turborepo (empezá simple)
- ❌ Múltiples apps (solo desktop primero)
- ❌ Remote wipe
- ❌ Workflow engine complejo
- ❌ UI de admin para gestionar control namespace (hacelo manual con scripts)
- ❌ Encriptación por namespace
