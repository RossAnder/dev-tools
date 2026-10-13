+++
id = "domain-model"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["persistence-leakage", "aggregate-boundaries", "migration-safety", "mapping-drift"]
activation = "signal"
slice = "per-subsystem"
apply_when = { signals = ["schema-change", "query-layer"] }
methods = ["trace"]
v1_category = "db"
+++

# domain-model

## Question

Do the domain types, the stored shape and the code mapping between them still agree, and does each change to stored data carry what existing data needs?

## Owns

`persistence-leakage` — storage concerns inside domain types: column names, nullable-for-the-database fields, ORM attributes or row ids driving domain logic; one type serving request, wire and storage roles so a change for one role silently changes the others.

`aggregate-boundaries` — aggregates that reference each other by object rather than by identity, so one load drags in another or one write mutates two; relationship cardinality in the model or schema that contradicts the domain.

`migration-safety` — a migration that changes stored shape without the data step existing rows need, that cannot run against populated tables, that drops or narrows data with no backfill, or that edits a migration already applied somewhere.

`mapping-drift` — columns, query results and mapped types that no longer agree: a selected column list missing a new field, a hand-written row decoder reading by stale position or name, a nullable column mapped to a non-optional type, a default in the schema that the code contradicts.

## Probes — code

Start from what the change does to stored shape or to the code that reads and writes it, then trace each affected type through every layer between the database and the caller.

- **Migrations.** For each migration in scope, state what existing rows look like before and after. Check that a new non-null column has a default or a backfill, that a type narrowing or rename preserves data, that the step is ordered after anything it depends on, and that no already-applied migration file was edited — a checksum-validated migration that changes breaks every database that ran it.
- **Mapping.** For each table or collection the change touches, list its columns and find every query, decoder, mapper and construction site that reads or writes it, using the trace method. Name each one that misses a column, reads a removed one, or maps nullability or type differently from the schema.
- **Roles.** For each type the change adds or alters, name which roles it plays: request input, wire output, domain logic, storage row. A type serving more than one role is a finding when the change needed a difference between roles and got it in all of them, or exposed a storage field on the wire.
- **Aggregates.** Follow references between domain types. Flag an object reference that crosses an aggregate boundary where an id would do, a write that changes two aggregates without one transaction or a stated reason, and a one-to-one or one-to-many shape the domain contradicts.
- **Defaults and constraints.** Compare schema defaults, unique constraints and foreign keys against what the code assumes; a uniqueness the code relies on with no constraint, or a cascade the code does not expect, is a defect with a nameable scenario.

## Does not flag

- A confirmation that a mapping or migration is correct; record it in the coverage note.
- A documented pragmatic shortcut — a shared request and storage type the project chose deliberately, a denormalisation with a stated reason. A disagreement is advisory with `angle = challenge`, naming the decision.
- Coupling confined to one module, or a layout no existing file contradicts.
- Query performance, indexing and fetch shape; those belong to the perf family.
- Type expressiveness unrelated to persistence; that is the type-design lens.
- A speculative future data volume or a hypothetical second database with no evidence in the scope.
- A remodelling whose gain is negligible against the migration risk it would carry.

## Edit forms

- `change` — bring a select list, decoder or mapped type back in line with the schema within the slice; correct a nullable mapping.
- `add` — the missing column at each construction site, listed under `instances`; a data step for a migration not yet applied anywhere.
- `remove` — a storage attribute or field from a domain type that nothing in the domain reads.

Never propose editing an applied migration; the fix is a new migration, and if it needs a decision it is advisory.

## Advisory angles

- `direction` — splitting a type that serves several roles, or moving an aggregate reference to an id, when callers outside the scope must follow.
- `risk` — a migration safe on today's data that fails on a state production may hold, stated with the state.
- `challenge` — a documented modelling choice the change now strains.

## Severity calibration

- `critical` — data loss or corruption on migration or write, an edited applied migration, or a mapping that writes wrong values to stored rows.
- `major` — a read that fails or returns wrong values for existing rows, or a migration that fails against a populated table.
- `minor` — drift that affects only a rarely populated column or a path that degrades gracefully.
