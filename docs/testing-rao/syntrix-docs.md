# syntrix-docs: wrapper de autorización para iroh-docs

## Qué hace

Es una capa fina (~300 líneas de Rust) entre tu app y iroh-docs. Su único trabajo es responder: **"¿este peer puede abrir este namespace?"**

NO maneja workflows, ni approvals, ni state machines. Solo acceso.

## Cómo funciona — versión minimalista

### 1. El namespace de control

Es un namespace normal de iroh-docs. Write: admin. Read: todos los empleados.

Lo que contiene:

```
org_control
├── "members/alice" → { author_id: 0xALICE, name: "Alice", active: true }
├── "members/bob"   → { author_id: 0xBOB,   name: "Bob",   active: true }
├── "roles/sales"       → { namespaces_read: ["products", "customers"], namespaces_write: ["invoices"] }
├── "roles/accounting"  → { namespaces_read: ["products", "customers", "payroll", "invoices"], namespaces_write: [] }
├── "roles/admin"       → { namespaces_read: ["*"], namespaces_write: ["*"] }
└── "assignments/alice" → "sales"
    "assignments/bob"   → "accounting"
```

### 2. Cuando un empleado inicia su app

```
Paso 1: App sincroniza org_control (todos tienen Read)
        → ya sabe qué roles existen y a quién están asignados

Paso 2: syntrix-docs lee "assignments/alice" → "sales"
        syntrix-docs lee "roles/sales" → { read: [products, customers], write: [invoices] }

Paso 3: syntrix-docs busca en su store local:
        ¿Tiene capability Read para "products"? → No → La pide al admin
        ¿Tiene capability Read para "customers"? → No → La pide al admin
        ¿Tiene capability Write para "invoices"? → No → La pide al admin
        ¿Tiene capability para "payroll"? → No, y su rol no la necesita → No se pide

Paso 4: Admin (o un proceso automático) genera los tickets y se los envía.
        Los tickets viajan por gossip o fuera de banda.

Paso 5: syntrix-docs importa los tickets. Ahora Alice tiene en su store local:
        - Read: products, customers
        - Write: invoices
```

### 3. Cuando llega una conexión de sync

```rust
// accept_cb: se ejecuta en CADA handshake de sync entrante
fn accept_cb(namespace: NamespaceId, peer: PublicKey) -> AcceptOutcome {
    let registry = self.registry.read();
    
    // ¿Tenemos capability para este namespace?
    if !registry.has_capability(&namespace) {
        // Ni siquiera lo intentamos sincronizar
        return AcceptOutcome::Reject(AbortReason::NotFound);
    }
    
    // ¿El peer que se conecta es un miembro activo?
    if !registry.is_active_member(&peer) {
        return AcceptOutcome::Reject(AbortReason::NotFound);
    }
    
    AcceptOutcome::Allow
}
```

### 4. Cuando un empleado es despedido

```
Admin actualiza org_control:
  "members/alice" → { active: false }

Próxima sync de org_control → todos los peers ven el cambio.
Próxima conexión de Alice → accept_cb ve "inactive" → Reject.
Alice no recibe más datos.

Sus datos históricos quedan en los peers que ya los tenían.
```

### 5. Diferencias con un CRDT normal

| Lo que iroh-docs hace | Lo que syntrix-docs agrega |
|---|---|
| Sync automático entre peers con capability | Decide QUIÉN recibe capability |
| Firma de entries (Author) | Mapea Author → Rol → Namespaces permitidos |
| Gossip en vivo | Bloquea peers inactivos a nivel handshake |
| Sin revocación | Shadow ban vía active/inactive |

## Lo que syntrix-docs NO es

| No es | Lo maneja |
|---|---|
| Un motor de workflows | Código de dominio en LiveStore |
| Un validador de reglas de negocio | Tu app |
| Un servidor central | El control namespace + admin peer |
| Un sistema de autenticación | Las capabilities criptográficas de iroh-docs |

## Tamaño estimado

```
syntrix-docs/
├── registry.rs       (~100 líneas) — lee control namespace, cachea roles
├── accept_cb.rs      (~50 líneas)  — decide si aceptar sync
├── capability.rs     (~80 líneas)  — pedir/importar tickets según rol
└── lib.rs            (~50 líneas)  — API pública
Total: ~300 líneas
```

Una sola responsabilidad: traducir "Alice es sales" a "Alice debe tener Read de products, customers y Write de invoices". Nada más.
