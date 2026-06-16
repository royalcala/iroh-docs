# iroh-docs para ERP multi-usuario

## El problema

Un ERP tiene múltiples roles (admin, contabilidad, ventas, depósito) y cada uno debe ver/editar distintos datos. iroh-docs tiene permisos **por namespace**, no por key ni por autor. Esto fuerza decisiones de diseño.

## Lo que iroh-docs NO puede hacer

- Permisos por fila dentro de un mismo namespace
- "El usuario X solo puede editar sus propias invoices"
- Revocar acceso a un usuario que ya tiene el secreto
- Impedir que un escritor autorizado modifique keys de otros

Lo que SÍ puede: firmar cada entry con el `Author`, probando quién la escribió. Pero probar != prevenir.

## Opciones analizadas

### Opción A — Un namespace por tenant, confianza total

```
Todos los usuarios comparten el NamespaceSecret.
Cualquiera puede escribir cualquier key.
El Author signature prueba quién escribió (audit), no previene.
```

❌ Sin seguridad real. Solo sirve si todos los usuarios son de plena confianza.

### Opción B — Un namespace por rol

```
invoices   → Write: ventas + contabilidad, Read: depósito
products   → Write: admin, Read: todos
payroll    → Write: RRHH, Read: RRHH + contabilidad
```

✅ Agrupa por quién escribe.  
⚠️ Dentro de `invoices`, todos los de ventas pueden modificar invoices ajenas.

### Opción C — Un namespace por usuario

```
invoices_user1 → Write: user1, Read: user1 + contabilidad
invoices_user2 → Write: user2, Read: user2 + contabilidad
invoices_user3 → Write: user3, Read: user3 + contabilidad
products       → Write: admin, Read: todos
```

✅ Aislamiento total por usuario.  
❌ Muchos namespaces (100 usuarios = 100+ namespaces).  
❌ "Mostrar todas las invoices" requiere abrir y mergear N namespaces.

### Opción D — Híbrida (recomendada)

| Tipo de dato | Namespace | Write | Read |
|---|---|---|---|
| **Catálogos compartidos** (productos, clientes, plan de cuentas) | `products`, `customers`, `chart_of_accounts` | admin | todos |
| **Transaccional por departamento** (invoices, órdenes de compra) | `invoices`, `purchase_orders` | departamento dueño | dueño + contabilidad |
| **Sensible por rol** (nómina, comisiones) | `payroll`, `commissions` | RRHH / admin | RRHH + contabilidad + el propio empleado |
| **Privado por usuario** (borradores, config personal) | `user_settings_<id>` | solo el usuario | solo el usuario |

### Opción E — App-level enforcement + audit

```
Un solo namespace grande.
La app valida antes de escribir (reglas de negocio).
El Author signature prueba quién escribió (audit posterior).
Si alguien viola las reglas, se detecta ex-post, no se previene.
```

⚠️ Solo viable con clientes controlados (no hay peers maliciosos).

## Recomendación final

**Opción D híbrida + enforcement en app.** Separás por dominio de escritura (quién puede modificar qué) y dentro de cada namespace confiás en que los escritores autorizados respetan las reglas. El `Author` signature da auditabilidad.

### Reglas de diseño

1. **Un namespace = un grupo de escritores con iguales privilegios.** Si dos roles tienen distintos permisos de escritura, van en namespaces distintos.

2. **Los catálogos maestros son Read para todos, Write para admin.** Nunca necesitás separar lectores — solo escritores.

3. **Para per-user isolation real, namespaces separados.** El costo es mayor cantidad de namespaces, pero redb lo soporta sin problema (miles de tablas en el mismo archivo).

4. **LiveStore mergea múltiples namespaces en SQLite.** Abre N namespaces, cada uno con su cursor, y materializa en tablas separadas o vistas unificadas.

5. **Si necesitás revocación**, rotá el namespace (nuevo `NamespaceId`) y distribuí nuevos tickets. Los viejos tickets quedan inválidos. Los datos viejos se migran o se abandonan.

### Ejemplo concreto: ERP con 3 roles

```
Namespaces:
───────────
products          Write: [admin]            Read: [admin, ventas, depósito]
customers         Write: [admin, ventas]    Read: [admin, ventas, depósito]
invoices          Write: [ventas]           Read: [ventas, contabilidad]
purchase_orders   Write: [depósito]         Read: [depósito, contabilidad]
payroll           Write: [RRHH]             Read: [RRHH, contabilidad]
config            Write: [admin]            Read: [admin]

Distribución de tickets:
────────────────────────
Usuario ventas:    Write(products? no), Read(products? sí), Write(customers),
                   Write(invoices), Read(purchase_orders)
Usuario depósito:  Read(products), Read(customers),
                   Write(purchase_orders), Read(invoices)
Usuario admin:     Write(products), Write(customers), Write(config),
                   Read(invoices), Read(purchase_orders), Read(payroll)
```

Cada usuario recibe múltiples tickets (o un ticket compound) con los capabilities que le corresponden. LiveStore en su dispositivo abre todos los namespaces que su rol cubre y construye las vistas SQLite correspondientes.
