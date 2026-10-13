+++
id = "data-shape"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["serialization-cost", "data-layout"]
activation = "candidate"
slice = "per-subsystem"
methods = ["measure"]
v1_category = "serialization"
research_domain = "databases and indexing for wire formats and encodings; systems and storage for serialization at the storage boundary"
+++

# data-shape

## Question

As data crosses the network, process and storage boundaries in this slice, is it shaped, encoded and transformed at the size, layer and format its use needs, or does the code pay for bytes, conversions and layouts nobody reads?

## Owns

`serialization-cost` — encoding, decoding and conversion work the data's use does not need: a payload carrying fields or nested records the consumer never reads, a value serialised and parsed again between two layers of one process, a full decode into owned structures where a borrowed or partial decode would serve, a document re-encoded whole to change one field, a transformation done on the wrong side of a boundary so more data crosses it than the result needs, and compression missing on a large transfer or applied to a small one.

`data-layout` — a data structure whose shape fights its access pattern: lookups by key over a sequence, a map iterated in order where a sorted or indexed structure fits, records stored as many small objects where the access reads one field across all of them, a format chosen for one access pattern but read through another, and schema choices whose evolution forces a full rewrite or a version-sniffing decode on every read.

## Probes — perf

Start from the workload, as the measure method requires: name the boundary, the payload or record count that crosses it and a size this repo really sees, then follow the data from producer to consumer. Climb the measure method's ladder and record the rung; without a PROFILE BASELINE fence or an existing benchmark, a defect needs a byte or conversion count or a complexity argument tied to the sourced size, and anything less is advisory with angle risk.

- **Payload against use.** For each payload the slice produces, list its fields and find which ones the consumers read. Name the fields, nested records or history sent and never used, and the size they add at the sourced count.
- **Round trips through a format.** Trace values that are serialised and parsed again between two layers that share a process or a type, and conversions applied twice on the same path, such as text to number to text.
- **Decode depth.** Where the consumer reads part of a large document or record, check whether the code decodes all of it into owned structures first, and whether a borrowed, streaming or partial decode exists for the format and library version pinned.
- **Which layer transforms.** Check whether filtering, projection, aggregation and sorting happen before the boundary or after it, and how much more data crosses than the result needs.
- **Layout against access.** For each collection the hot path reads, name the access pattern — by key, in order, by range, one field across all records — and check the structure serves it without a scan or a conversion.
- **Format and evolution.** Check whether the chosen format fits the access pattern, whether compression is applied where the payload is large and absent where it is small, and whether a schema change forces every reader to re-decode or every stored record to be rewritten.

Serialisation guidance shifts between library versions: verify every claim about a library's decoding, borrowing or compression behaviour against the version pinned in the manifest, not the ecosystem in general. The facets bound to the cell carry the stack's own pitfalls; apply them alongside these probes.

## Does not flag

- A confirmation. A payload you traced and found lean goes in the cell's coverage note, never in a finding.
- A payload or structure whose size at the sourced workload makes the saving one a user would not notice.
- A hypothesis without a sourced size or count, raised as a defect; it is advisory with angle risk, naming the measurement that would settle it, or nothing.
- A claim about a serialisation library's behaviour you did not check against the pinned version, or a claim resting on a comment you did not verify against the code.
- A format or wire contract the project documents as deliberate or fixed by an external consumer — a public API shape, a stored format with readers outside the scope. A disagreement is advisory with angle challenge or alternative, naming the decision.
- Query shape, round trips to a database and caching of reads; those belong to the data-access lens. Super-linear algorithms belong to the algorithm lens, and allocation unrelated to encoding to the memory-runtime lens.
- Whether a type models the domain correctly — illegal states, newtypes, redundant representations; that is the code family's type-design lens. Frame findings here by access pattern and cost.
- A format swap whose gain is negligible against the migration and compatibility risk, or a speculative future scale with no evidence in the scope.
- Anything the LINT BASELINE already reports, and work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

Cleanup here is the small, safe win inside the slice that changes no contract a consumer outside it reads.

- `change` — decode into a borrowed or partial form where the consumer reads a part; move a filter or projection before the boundary when both sides are in the slice; replace a scan for a key with a keyed structure built once.
- `remove` — a serialise-then-parse round trip between layers of one process; a field computed and sent that no consumer in the tree reads.
- `add` — compression or a size-appropriate option on a large internal transfer when the library pinned already supports it.

## Advisory angles

- `risk` — a cost the shape makes plausible but no rung above hypothesis supports, with the measurement that would settle it.
- `alternative` — a different format, encoding or storage layout whose adoption changes a contract or a stored format.
- `direction` — moving transformation work across a boundary when the other side lies outside the scope.
- `challenge` — a documented format or wire contract that a sourced workload now strains.

## Severity calibration

- `critical` — a payload or decode whose cost grows with stored data so a user-facing operation fails or times out at the sourced size, or a format change that forces rewriting every stored record on read.
- `major` — a measured or complexity-argued cost a user would notice at the sourced size: whole-document decodes on a per-request path, payloads several times larger than their use, a layout that turns keyed access into a scan over a large collection.
- `minor` — a real, counted cost on a hot path whose magnitude at the sourced size is small but not negligible.
