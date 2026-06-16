# Síntesis: ERP P2P sobre iroh-docs

> **Nota:** Este documento analiza la discusión teórica con 3 modelos de IA. Para la decisión final de arquitectura, ver [Decisión de arquitectura](decision.md).

## Lo que los tres modelos coinciden

1. **El requisito 8 es la restricción dura.** "Cada peer solo debe recibir lo que su rol le permite ver" implica que un namespace compartido donde todos escriben y todos leen es imposible. El sync baja todo el namespace, sin filtro.

2. **Más namespaces, no menos.** La frontera de seguridad real es el `Capability`, no la validación app-level.

3. **Namespace de control es esencial.** Admin escribe, todos leen. Contiene la lista de miembros, roles, y qué namespaces cada rol puede abrir.

4. **Los datos históricos sobreviven al empleado.** Lo que Alice escribió pertenece a la org, no a Alice.

## Lo que cada uno aporta de único

| Modelo | Aporte clave |
|---|---|
| **ChatGPT** | "¿Cuántos namespaces soporta iroh-docs? Si son 10k, dejá de luchar contra el modelo." Namespaces por lote transaccional (`tx_alice_2026_q1`). |
| **Claude** | Variante del **conductor/agente**: el NamespaceSecret de los namespaces compartidos lo tiene un peer privilegiado, no todos los empleados. Los empleados envían "intents" efímeros. Cuando Alice sale, el conductor simplemente deja de aceptar sus intents. Alice **nunca tuvo el secreto**. |
| **Gemini** | **Bloqueo a nivel de red** vía `NodeId`: además de los capabilities, el admin bloquea el `EndpointId` de Alice para que ni siquiera pueda conectarse al swarm. Revocación en 3 capas: red, datos, seguridad. Y señala el problema de **exfiltración de lectura**: si Alice tiene el `NamespaceId` de catálogos, puede seguir leyendo para siempre vía cualquier peer que no la bloquee. |

## El diseño sintetizado

```
┌─ Capa de red ─────────────────────────────────────┐
│  NodeId allowlist en cada peer                     │
│  (Gemini: bloquear conexiones de ex-empleados)     │
├─ Capa de autorización ────────────────────────────┤
│  Conductor/Agency (Claude: peer privilegiado)      │
│  - Tiene los NamespaceSecrets de la org            │
│  - Recibe intents de empleados vía gossip efímero  │
│  - Valida author ∈ valid_authors                   │
│  - Escribe al namespace con su Author              │
│  - Empleados NUNCA tienen los secretos de la org   │
├─ Namespaces (ChatGPT + Gemini: muchos, chicos) ──┤
│  org_control        Write: conductor  Read: todos  │
│  org_products       Write: conductor  Read: todos  │
│  org_customers      Write: conductor  Read: todos  │
│  org_payroll        Write: conductor  Read: HR+cont│
│  tx_alice_2026_q1   Write: alice      Read: contab │
│  tx_bob_2026_q1     Write: bob        Read: contab │
│  tx_alice_2026_q2   Write: alice      Read: contab │
│  ...                                               │
├─ LiveStore ───────────────────────────────────────┤
│  Abre N namespaces según org_control               │
│  Materializa vistas unificadas en SQLite           │
│  Cursor por namespace (PR #108)                    │
└───────────────────────────────────────────────────┘
```

## Flujo de escritura

```
Empleado Alice (ventas)
  │
  │  1. Alice crea una invoice en su UI
  │  2. LiveStore genera el intent: { author: alice, key: "invoice/001", data: {...} }
  │  3. Envía el intent vía gossip efímero al conductor
  │
  ▼
Conductor (peer privilegiado)
  │
  │  4. Valida: ¿alice ∈ valid_authors? ¿role = sales?
  │  5. Escribe al namespace tx_alice_2026_q2 con Author = conductor
  │     (o preserva Author = alice para auditoría)
  │  6. El NamespaceSecret de tx_alice_2026_q2 nunca sale del conductor
  │
  ▼
Contabilidad (Read de tx_alice_2026_q2)
  │
  │  7. Recibe la nueva entry vía sync/gossip
  │  8. LiveStore la materializa en SQLite
```

## Flujo de despido

```
Admin despide a Alice
  │
  │  1. Admin elimina alice de valid_authors en org_control
  │  2. Admin elimina alice_node_id de active_nodes
  │
  ├─► Conductor: deja de aceptar intents de Alice
  │
  ├─► Red (todos los peers): rechazan conexiones del NodeId de Alice
  │
  ├─► Datos históricos: intactos en tx_alice_2026_q1 y tx_alice_2026_q2
  │
  └─► Alice: no puede escribir (conductor rechaza), no puede leer (red bloqueada)
       Sus datos locales son históricos muertos.
```

## Preguntas abiertas

1. **¿Cuántos namespaces soporta iroh-docs sin degradación?** Si la respuesta es 1000+, el diseño de ChatGPT (muchos chicos) es viable. Si es 100, toca agrupar por períodos más grandes.

2. **¿El conductor es single-point-of-failure para escrituras?** Sí. En un ERP esto es aceptable (las escrituras requieren connectivity de todas formas). Se puede mitigar con conductor redundante (2-3 peers con el secreto).

3. **¿Rotación de namespaces transaccionales?** Cada trimestre se crea un namespace nuevo. Los viejos quedan en modo archivo (Read-only para contabilidad). Alice se va en Q3, sus namespaces Q1 y Q2 son históricos, Q3 nunca existió para ella.

4. **¿Gossip efímero para intents?** Requiere un topic de gossip separado donde el conductor escucha. Los intents no son entries — son mensajes efímeros. Si se pierden, el empleado reenvía.
