# Decisión de arquitectura: ERP sobre iroh-docs

## Filosofía

Buscamos **la solución más simple que resuelva el 90% de los casos reales**, no una arquitectura invulnerable a todo escenario teórico. Cada capa adicional que agregamos tiene costo de mantenimiento. Solo la agregamos si el riesgo que mitiga es real en nuestro modelo de negocio.

## Qué queremos (y qué no)

| Sí necesitamos | No necesitamos (por ahora) |
|---|---|
| Sync P2P entre sucursales sin servidor | Defensa contra empleados maliciosos con acceso físico |
| Separación de datos por rol (ventas no ve nómina) | Encriptación por namespace contra peer cómplice |
| Auditoría: saber quién escribió qué | VPN entre sucursales |
| Offline-first: escribir sin internet | Rotación automática de namespaces |
| Funciona en Tauri (Windows, macOS, Linux, mobile) | MDM / wipe remoto |

## Arquitectura final: namespace por escritor, merge en lectura

**Decisión:** Cada empleado escribe a su propio namespace transaccional. Los lectores mergean usando `UNION ALL` en SQLite. Los catálogos compartidos los escribe solo admin. Nadie comparte Write con nadie. No hay revocación que hacer porque nunca se dio Write ajeno.

```
Namespaces:

Catálogos (1 escritor: admin, N lectores)
├── org_products         Write: admin      Read: todos
├── org_customers        Write: admin      Read: todos
└── org_chart_accounts   Write: admin      Read: todos

Transaccional (1 escritor por namespace, 2-3 lectores)
├── invoices_alice       Write: alice      Read: admin, contabilidad
├── invoices_bob         Write: bob        Read: admin, contabilidad
└── ...

Privado (1 escritor, 1 lector)
├── user_alice           Write: alice      Read: alice, admin
└── user_bob             Write: bob        Read: bob, admin

Sensible (1 escritor, pocos lectores)
└── org_payroll          Write: admin      Read: admin, HR, contabilidad
```

**[syntrix-docs](syntrix-docs.md)** (~300 líneas) envuelve iroh-docs y maneja:
- Leer `org_control` para saber roles y miembros activos
- `accept_cb`: rechazar syncs de peers inactivos o namespaces no autorizados
- Distribuir tickets de lectura según rol

```
┌─ Tauri App ──────────────────────────────────────────────┐
│                                                           │
│  Rust backend                                             │
│  ├── iroh (P2P)                                           │
│  ├── iroh-docs (storage + sync)                           │
│  │   └── syntrix-docs (autorización)                      │
│  └── LiveStore → SQLite                                   │
│       └── invoices_view =                                 │
│           SELECT * FROM invoices_alice                    │
│           UNION ALL                                       │
│           SELECT * FROM invoices_bob                      │
│           UNION ALL ...                                   │
└───────────────────────────────────────────────────────────┘
```

### Por qué esta arquitectura

| Problema | Cómo lo evitamos |
|---|---|
| Revocación de Write | Nadie comparte Write. Cada empleado tiene Write solo en su namespace. |
| Rotación de namespaces | Innecesaria. Si Alice se va, `invoices_alice` queda como archivo. |
| Conductor / SPOF | Innecesario. Cada empleado escribe directo. |
| Datos duplicados | Cada dato vive UNA vez. LiveStore mergea en lectura. |
| Cambio de rol | Solo afecta Read (agregar/quitar tickets de lectura). |

## Qué resuelve cada capa

| Capa | Problema que resuelve | Cómo |
|---|---|---|
| `org_products` (Read para todos) | Datos compartidos visibles para toda la org | Ticket Read distribuido a todos los empleados |
| `org_payroll` (Read solo HR+contab) | Datos sensibles invisibles para ventas | Ticket Read solo para HR y contabilidad. Ventas ni siquiera conoce el NamespaceId |
| `accept_cb` en sync | Un peer sin capability no puede sincronizar | Rechazo a nivel de handshake, antes de transferir datos |
| Author signatures | Auditoría: quién escribió qué y cuándo | Cada entry firmada por el Author del empleado |
| Control namespace | Admin decide quién tiene acceso a qué | Entries con lista de miembros activos y sus roles |

## Lo que NO resuelve (y está bien)

| Escenario | Por qué no lo resolvemos | Quién lo maneja |
|---|---|---|
| Empleado se va con la laptop | La empresa recupera el hardware | IT / RRHH |
| Empleado filtra datos mientras trabaja | Puede hacer screenshot, copiar a USB, etc. | Política de seguridad / RRHH |
| Peer cómplice reenvía datos a ex-empleado | Mismo vector que reenviar un email | Contrato laboral / auditoría |
| Ataque externo a la red P2P | iroh usa QUIC encriptado end-to-end | iroh (no es nuestro problema) |

## Multi-org

Un empleado puede pertenecer a varias organizaciones. La misma laptop, el mismo `docs.redb`. Solo se prefijan los namespaces con el ID de la org:

```
docs.redb de Alice

Org ACME (Alice es admin, dueña):
├── acme::control              Write: alice    Read: empleados_acme
├── acme::products             Write: alice    Read: empleados_acme
├── acme::invoices_bob         Write: bob      Read: alice, contabilidad_acme
└── acme::user_alice           Write: alice    Read: alice

Org BETA (Alice es empleada):
├── beta::control              Write: admin_beta  Read: empleados_beta
├── beta::products             Write: admin_beta  Read: empleados_beta
├── beta::invoices_alice       Write: alice       Read: admin_beta, contabilidad_beta
└── beta::user_alice           Write: alice       Read: alice
```

**El mecanismo es igual.** `syntrix-docs` itera sobre todos los control namespaces que conoce. Un despido en BETA solo afecta los namespaces `beta::*`. Los de ACME no se tocan.

```
Alice despedida de BETA:
  ├── beta::control → "active: false"
  ├── beta::products, beta::invoices_alice → accept_cb bloquea sync
  └── acme::* → intactos, Alice sigue siendo admin de ACME
```

## Remote wipe (opcional, etapa futura)

Para dispositivos de la empresa, se puede agregar una señal de "borrado remoto" en el control namespace. Cuando el peer la detecta, elimina todos los datos de esa org de su `docs.redb`.

```
Control namespace:
  "members/alice" → { active: false, remote_wipe: true }

Al recibir remote_wipe:
  1. syntrix-docs detecta la señal en el control namespace
  2. Cierra todos los namespaces de esa org
  3. Llama a store.remove_replica() para cada namespace
  4. Los datos de la org desaparecen del docs.redb local
  5. Los datos de OTRAS orgs no se tocan

Limitación:
  ❌ Solo funciona si la máquina está online para recibir la señal
  ❌ Un empleado puede desconectar la máquina antes del wipe
  ❌ No reemplaza el cifrado de disco ni el control físico del hardware
```

Esto es opcional. En la versión inicial, el despido solo bloquea sync futuro (los datos viejos quedan). El remote wipe se puede agregar después si el caso de uso lo requiere.

## Casos de uso cubiertos

### Caso 1: Nuevo empleado

```
1. Admin crea Author para el empleado (o el empleado crea el suyo)
2. Admin agrega { author_id, role: "sales" } al control namespace
3. Admin genera tickets Read para: org_products, org_customers
4. Admin genera ticket Write para: user_nuevo (namespace personal)
5. Empleado importa tickets → sincroniza → listo
```

### Caso 2: Empleado cambia de rol

```
1. Admin actualiza role en control namespace (ej: "sales" → "manager")
2. Empleado ya tenía Read de org_products, org_customers (no cambia)
3. Admin puede agregar Read de org_payroll si el nuevo rol lo requiere
4. Admin genera ticket nuevo para org_payroll → empleado importa
```

### Caso 3: Empleado despedido (detallado)

```
Día 1 — Alice trabaja normalmente.

  Alice tiene en su store local:
    Read:  org_products, org_customers
    Write: invoices_alice, user_alice

  Control namespace:
    "members/alice" → { active: true, role: "sales" }


Día 2 — Alice es despedida.

  Admin actualiza control namespace:
    "members/alice" → { active: false }

  Esto se sincroniza automáticamente a todos los peers vía gossip.


Día 3 — Qué pasa en cada peer.

  PEER DE ALICE (laptop de la empresa):
    └── Su NamespaceRegistry lee control → "active: false"
    └── Cierra org_products y org_customers (deja de intentar sync)
    └── invoices_alice y user_alice siguen abiertos (son suyos)
    └── No recibe más actualizaciones de la org

  PEER DE BOB (sigue activo):
    └── Su accept_cb recibe conexión de Alice
    └── Consulta control → "active: false"
    └── Reject — no acepta sync entrante de Alice
    └── Alice no puede enviar ni recibir datos de Bob

  PEER DE CONTABILIDAD:
    └── invoices_alice sigue abierto (Read)
    └── Datos históricos de Alice intactos y legibles
    └── Nadie escribe a invoices_alice → queda como archivo histórico


Qué logramos:
  ✅ Alice deja de recibir datos nuevos de la org
  ✅ Sus datos históricos quedan disponibles para la org
  ✅ Su Write de invoices_alice es irrelevante (nadie lo usa)
  ✅ Sin rotación de namespaces, sin conductor

Qué NO logramos (limitación honesta):
  ❌ Si Alice tiene una laptop PERSONAL con datos ya sincronizados,
     puede seguir viendo esos datos viejos (igual que emails viejos)
  ❌ Si Alice modifica el cliente para ignorar el control namespace,
     puede seguir intentando leer. Los demás peers la rechazan, pero
     si algún peer no aplica bien el accept_cb, Alice puede colarse.
     → Esto se mitiga con bloqueo a nivel NodeId (capa de red)
```

### Caso 4: Dos sucursales sin servidor central

```
Sucursal A                          Sucursal B
──────────                          ──────────
Peer Admin_A (conductor)            Peer Admin_B (conductor)
  │                                   │
  ├── org_products (Write)            ├── org_products (Write) ← ambos pueden
  ├── org_customers (Write)           ├── org_customers (Write)
  └── user_alice (Write)              └── user_bob (Write)

Ambos conductores tienen Write en los namespaces compartidos.
Los cambios se sincronizan automáticamente vía set reconciliation + gossip.
No hay conflicto: cada sucursal escribe keys distintas o LWW resuelve.
```

## Lo que NO hacemos (todavía)

| Idea | Por qué no |
|---|---|
| Encriptación por namespace | Agrega complejidad de key management. Solo necesaria si hay amenaza de peer malicioso. |
| Rotación de namespaces al despedir empleados | Doloroso, innecesario si el hardware es de la empresa. |
| Entry validation hook en sync | Útil, pero el accept_cb a nivel namespace ya cubre el 90%. Se puede agregar después si hace falta. |
| VPN entre sucursales | iroh ya resuelve conectividad P2P segura. VPN sería redundante y con más costo de mantenimiento. |
| Conductor como peer privilegiado | Agrega un single point of failure para escrituras. Solo necesario si MUCHOS empleados escriben al mismo namespace compartido. Si cada empleado escribe a su `user_*` namespace y los compartidos son read-only para todos menos admin, no hace falta. |

## Costo total

| Componente | Quién lo mantiene | Costo |
|---|---|---|
| iroh | n0-computer (upstream) | $0 |
| iroh-docs | n0-computer + nuestro fork para el cursor | $0 (contribución open source) |
| Control namespace | Nosotros (300 líneas de Rust) | Una vez |
| LiveStore → SQLite | Nosotros (ya existe) | Ya hecho |
| Tauri shell | Tauri (upstream) | $0 |
| Infraestructura | Ninguna (P2P, sin servidores) | $0 |
| Relay servers | iroh relays públicos o self-hosted | $0 o costo de VPS mínimo |

**Costo operativo mensual: $0** (sin servidores, sin VPN, sin DB central). Solo el costo de mantener el código.
