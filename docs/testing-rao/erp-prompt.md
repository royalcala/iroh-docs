# Prompt para analizar con otros modelos de IA

## Contexto

Estamos diseñando un ERP descentralizado P2P usando **iroh-docs** como capa de storage y sync.
iroh-docs es un key-value store sincronizable con las siguientes características:

- La unidad de datos es el **Namespace** (documento), identificado por un `NamespaceId` (32 bytes, clave pública ed25519)
- Cada namespace tiene un `NamespaceSecret` (clave privada). Quien tiene el secreto puede **escribir**. Quien solo tiene la clave pública (NamespaceId) puede **leer**.
- Los permisos son binarios por namespace: Write (tiene el secreto) o Read (solo público). No hay permisos por key, por autor, ni granularidad intermedia.
- **No hay revocación**: si alguien tuvo Write en el pasado, conoce el secreto para siempre. La única forma de "revocar" es crear un namespace nuevo y migrar datos.
- Los datos se distribuyen vía **set reconciliation + gossip** P2P. Todo peer con acceso a un namespace eventualmente recibe todas sus entries.
- Las entries se firman con dos pares de claves: `NamespaceSecret` (permiso) + `Author` (identidad del escritor). Esto permite auditar quién escribió qué, pero no prevenir que escriba.
- El storage local es `redb` (embedded B-tree). Un solo archivo `docs.redb` contiene todos los namespaces.

## Requisitos del ERP

1. **Múltiples organizaciones (orgs).** Un usuario puede pertenecer a varias orgs.
2. **Roles dentro de cada org:** admin, ventas, contabilidad, depósito, RRHH.
3. **Permisos dinámicos:** los roles cambian, empleados entran y salen.
4. **Datos compartidos (catálogos):** productos, clientes, plan de cuentas — visibles para toda la org.
5. **Datos transaccionales con dueño:** cada invoice pertenece a un empleado. Ese empleado la edita, contabilidad la lee, otros empleados NO.
6. **Datos sensibles por rol:** nómina solo visible para RRHH y contabilidad.
7. **Cuando alguien sale de la org, sus datos históricos deben seguir visibles para la org.**
8. **Cada peer solo debe recibir los datos que su rol le permite ver.** No sirve filtrar post-recepción porque el dato ya está en su disco.
9. **El sistema debe funcionar sin servidor central** (los peers se sincronizan directamente).

## Restricción adicional

Usamos **LiveStore/LiveSQL** que materializa los namespaces en SQLite local para consultas relacionales. Puede mergear múltiples namespaces en vistas SQL.

## Lo que ya analizamos

- Crear namespaces por rol (`invoices`, `payroll`, `products`) con Write para el rol dueño y Read para los roles que necesitan ver.
- El problema: dentro de `invoices`, todos los del rol ventas pueden modificar invoices ajenas.
- Crear namespaces por usuario (`alice_invoices`, `bob_invoices`) con Write solo para el dueño y Read para contabilidad.
- El problema: muchos namespaces (N usuarios = N+ namespaces), y cuando Alice sale, sus datos quedan en `alice_invoices` pero ella sigue teniendo el secreto (no hay revocación).

## Pregunta

¿Cómo modelar namespaces, capabilities y flujo de datos para que un ERP multi-org, multi-rol, con rotación de personal funcione sobre iroh-docs sin servidor central?

## Mi análisis (borrador)

Creo que la respuesta más pragmática no está en multiplicar namespaces por usuario, sino en separar dos conceptos que están mezclados:

**1. El namespace NO es del usuario. Es de la organización.**

Los datos los crea el empleado, pero pertenecen a la org. El Author signature prueba quién los escribió, no quién es dueño. Si Alice se va, las invoices que generó son de la org.

**2. La validación de escritura es app-level, la sync es protocol-level.**

La pregunta no es "¿puede Alice firmar entries?" (siempre puede, tiene Author). La pregunta es "¿los demás peers aceptan sus entries?"

Esto requiere un hook nuevo en iroh-docs: antes de aceptar una entry en sync, validar contra una lista de autores permitidos. Esa lista vive en un namespace de control (Write: admin, Read: todos).

```
Estructura propuesta:
─────────────────────

org1_control          Write: admin     Read: toda la org
  → "valid_authors" = [alice_id, bob_id, carol_id]
  → "org_roles" = { alice: sales, bob: accounting }

org1_products         Write: admin     Read: toda la org
org1_customers        Write: admin     Read: toda la org

org1_transactions     Write: admin     Read: toda la org
  → admin escribe en nombre de todos
  → el Author de la entry identifica al empleado real
  → o: cada empleado escribe con su Author, validado contra valid_authors

org1_payroll          Write: admin     Read: HR + accounting

Cuando Alice sale:
  1. Admin borra alice_id de valid_authors
  2. La sync se encarga de propagar el cambio
  3. Los peers rechazan nuevas entries de Alice
  4. Las entries históricas de Alice quedan (son propiedad de la org)
  5. Alice sigue teniendo el NamespaceSecret, pero nadie acepta sus entries nuevas
```

No es perfecto: Alice podría escribir a su `docs.redb` local. Pero esas entries nunca llegan a nadie. Es un shadow ban criptográfico.


