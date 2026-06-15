# Lo que LiveStore necesita del sync

## Lo que almacena localmente

LiveStore usa **dos bases SQLite** locales:

| DB | Contenido | Dónde se define |
|----|-----------|----------------|
| Event Log DB | Tabla `eventlog`: todos los eventos con PK compuesta `(seqNumGlobal, seqNumClient, seqNumRebaseGeneration)` + `argsJson`, `name`, `clientId`, `sessionId`, `syncMetadataJson` | `eventlog-tables.ts:21-43` |
| Event Log DB | Tabla `__livestore_sync_status`: el `head` (último seqNum global visto del backend) y `backendId` | `eventlog-tables.ts:47-60` |
| State DB | Tablas materializadas (tus datos: invoices, users, etc.) + `sessionChangeset` para rollback | Definido en el schema del usuario |

El PK compuesto permite eventos locales pendientes (`seqNumClient > 0`) y rebases.

## Lo que va por el wire (SyncBackend.push / pull)

El formato que viaja entre cliente y backend es **simplificado respecto al formato interno**:

```ts
// Wire format: LiveStoreEvent.Global.Encoded
{
  name: "facturaCreated",      // string
  args: { id: "1", ... },      // any (el payload del evento)
  seqNum: 42,                   // integer (global sequence number)
  parentSeqNum: 41,             // integer (evento padre)
  clientId: "client-abc",       // string
  sessionId: "session-123"      // string
}
```

`global.ts:19-26` — **Solo integers para seqNum y parentSeqNum.** Nada de composite PK ni rebase generation.

## Cursor del pull

El cursor que LiveStore pasa al backend:

```ts
// Option de:
{
  eventSequenceNumber: number,     // integer, ej: 42
  metadata: Option<JsonValue>      // metadatos opcionales del backend
}

// Option.none() → pull desde el primer evento
// Option.some({ eventSequenceNumber: 42 }) → pull eventos DESPUÉS del seqNum 42
```

`sync-backend.ts:50-55` y `ClientSessionSyncProcessor.ts` (cursor se deriva del `upstreamHead.global`)

## Payload del push

```ts
push(batch: LiveStoreEvent.Global.Encoded[])

// 1-100 eventos por batch
// seqNum en orden ascendente
// El backend debe validar que seqNum > head actual
```

`sync-backend.ts:66-73`

## Respuesta del pull

```ts
PullResItem = {
  batch: [
    {
      eventEncoded: LiveStoreEvent.Global.Encoded,  // wire format
      metadata: Option<JsonValue>                    // opcional
    }
  ],
  pageInfo:
    | { _tag: 'NoMore' }           // no hay más eventos, pull terminó
    | { _tag: 'MoreUnknown' }      // hay más, cantidad desconocida
    | { _tag: 'MoreKnown', remaining: number }  // hay más, cantidad conocida
}
```

`sync-backend.ts:162-168`

## Errores que el backend debe emitir

| Error | Cuándo |
|-------|--------|
| `IsOfflineError` | Sin conectividad |
| `BackendIdMismatchError` | El cliente habla con un backend diferente al esperado |
| `ServerAheadError` | El cliente envía eventos con seqNum que el backend ya tiene |
| `UnknownError` | Cualquier otro error |

## Qué significa esto para Iroh Docs

El mapeo es directo:

| Contrato LiveStore | Iroh Docs |
|-------------------|-----------|
| `seqNum` (integer) | HLC + node_id como key ordenable (`evt:<hlc>:<node>`) |
| `push(batch)` | `doc.setBytes("eventlog/evt:<hlc>:<node>", JSON.stringify(event))` |
| `pull(cursor)` | Iterar keys con prefijo `eventlog/` desde el HLC del cursor |
| `cursor.eventSequenceNumber` | El último HLC procesado |
| `pageInfo` | Si quedan más entradas en el prefijo |
| `metadata` | Se puede guardar en el valor del evento como campo extra |
| `live: true` | Suscripción a cambios en el doc (nuevas keys en el prefijo) |

El `seqNum` de LiveStore (integer) se reemplaza por nuestro HLC (ts + count + node). El cursor pasa de ser un integer a ser un HLC serializado. El resto del contrato se cumple igual.
