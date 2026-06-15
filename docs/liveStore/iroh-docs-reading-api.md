# API de lectura de iroh-docs

## Métodos expuestos en `Doc` (`api.rs`)

| Método | Qué hace | ¿Cursor? |
|---|---|---|
| `get_exact(author, key, include_empty)` | Busca UNA key exacta con un author dado | No — devuelve una sola entry |
| `get_many(query)` | Devuelve un stream de entries que matchean el query | No — el cursor es client-side con offset |
| `get_one(query)` | Igual que get_many pero devuelve solo la primera | No |

## El Query builder (`store.rs`)

El query se construye con:

```rust
let stream = doc.get_many(
    Query::all()
        .key_prefix("evt:")              // filtrar por prefijo
        .author(alice_id)                // filtrar por autor
        .sort_by(SortBy::KeyAuthor, Asc) // ordenar por key
        .limit(100)                      // máximo de resultados
        .offset(0)                       // saltar N resultados
        .include_empty()                 // incluir entradas vacías
).await?;
```

## Qué filtros existen hoy (`KeyFilter`)

| Variant | Qué permite | Ejemplo |
|---|---|---|
| `Any` | Todas las keys | `Query::all()` |
| `Exact(Bytes)` | Una key exacta | `Query::key_exact("evt:001")` |
| `Prefix(Bytes)` | Keys que empiezan con X | `Query::key_prefix("evt:")` |

**No existe** un variant que diga "keys con prefijo X, pero desde la posición Y en adelante".

## Dónde se agregaría

Son 3 archivos, todos en `src/`:

| Archivo | Qué se agrega |
|---|---|
| `store.rs` (línea 359) | Nuevo variant `KeyFilter::PrefixFrom { prefix, from }` |
| `store.rs` (línea 187) | Nuevo método `QueryBuilder::key_prefix_from(prefix, from)` |
| `store/fs/bounds.rs` (línea 106) | Manejar `PrefixFrom` al construir los bounds del B-tree |

## Cómo quedaría

```rust
// KeyFilter actualizado
pub enum KeyFilter {
    Any,
    Exact(Bytes),
    Prefix(Bytes),
    PrefixFrom { prefix: Bytes, from: Bytes },  // ← NUEVO
}

// QueryBuilder — nuevo método
pub fn key_prefix_from(mut self, prefix: impl AsRef<[u8]>, from: impl AsRef<[u8]>) -> Self {
    self.filter_key = KeyFilter::PrefixFrom {
        prefix: prefix.as_ref().to_vec().into(),
        from: from.as_ref().to_vec().into(),
    };
    self
}

// Uso desde LiveStore
let stream = doc.get_many(
    Query::all()
        .sort_by(SortBy::KeyAuthor, SortDirection::Asc)
        .key_prefix_from("evt:", last_processed_hlc)
        .limit(100)
).await?;
```

El `from` es el último HLC que LiveStore ya procesó. El B-tree de redb hace seek directo a esa posición y solo devuelve eventos nuevos. No se saltea nada en cliente.
