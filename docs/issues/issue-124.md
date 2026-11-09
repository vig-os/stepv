---
type: issue
state: open
created: 2026-11-08T19:54:54Z
updated: 2026-11-08T19:58:00Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/124
comments: 1
labels: discussion, feature
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-11-09T09:29:48.692Z
---

# [Issue 124]: [spike: stepv as an agent tool (CLI + MCP): queryable STEP graph, measurements, rendered views, rolling cache, vigil request log + replay bench](https://github.com/vig-os/stepv/issues/124)

Owner idea (2026-11-08): give agents tools to **investigate a STEP file without reading it into context**. They walk and query the model's graph (jq-like), ask for volumes, arrangements and "what is inside what", measure, and get rendered views to look at (vision models). Behind it sits a cache, so repeated calls don't re-parse the file, and vigil logs every request, which also serves as a benchmark corpus.

## Motivation
- A STEP file is megabytes of entity soup. An agent reading it raw burns context and still can't answer geometric questions: volume, containment, clearance, "what's in this subassembly".
- stepv already has the expensive parts:
  - an OCCT kernel with a memory cap and a sandbox (#23);
  - the product tree and topology (`--topology` JSON, #26);
  - mesh streaming;
  - an offscreen viewer.
- #115's graph model (mmap, CSR edges, lazy arguments, bounding boxes per node, culling by containment) is exactly the index such a query surface needs.
- The decision this issue should settle is **the tool surface, the cache architecture and the honesty contract**, before #115's graph core is built, so that the graph is designed to serve queries, not only rendering.

## Proposed approach (a draft for the review to refute)
### Surface: one core, two front ends
- **`stepv q`** (CLI) and **`stepv mcp`** (stdio MCP server) call the same library API. The CLI comes first: agents without MCP, scripts and tests use it too.
- **`open(path)`** returns a **handle** (the file's content hash). Every other tool takes the handle, never the path or the bytes.
- **Tools, a first cut:**

  | Area | Tools |
  |---|---|
  | Overview | `summary` (units, entity counts, product tree depth, bounding box) |
  | Product tree | `tree`/`children` (paginated, depth-limited); `find` (by name or regex, type, attribute) |
  | Query | `query`: a jq expression over a lazily materialised JSON view of the graph. Use the **`jaq` crate**; don't invent a language. |
  | Measure | `measure`: volume, area, centroid, mass properties, axis-aligned and oriented bounding boxes; per definition vs per instance stated explicitly |
  | Relations | `relations`: contains / inside (#115 bounding boxes + winding numbers), touching / interference, shared-edge adjacency, minimum distance between parts |
  | Views | `render`: view or camera, highlight/isolate/hide-internals, section plane, size, returning a PNG file path plus an optional inline image under a byte cap |
  | Export | `export`: a subset as a mesh or STEP |

- **Honesty contract (#115):** every answer carries its **tier and precision**: proxy (bounding box) vs tessellated vs exact (OCCT), with tolerances and units. `exact=true` escalates, at a reported cost. An undefined answer is an error that gives its reason, never a guessed number. Examples: the volume of an open shell, a non-manifold solid.
- **Budget:** every call has a time and memory budget (#83's `stepv-cap`) and an output size cap, with pagination.

### Cache: a rolling window
- A **per-user daemon** (a unix socket; the CLI spawns it if absent; it exits when idle) holds parsed graphs and meshes. The key is (content hash, kernel version, options).
- It evicts least-recently-used entries within a memory budget, the "rolling window", and is admission-controlled like the Quick Look supervisor (#108).
- Results that are expensive to compute (exact volumes, renders) are memoised **on disk**, keyed by (handle, tool, canonical arguments). A cold start reloads the mmapped graph file instead of re-parsing.
- **Relation to gerchowl/mcpx** (which already has a call cache, handles and vigil audit): stepv owns the **domain** cache (graphs, meshes, geometric results), and mcpx can sit in front for the generic call cache. Don't duplicate one inside the other.

### Logging and benchmarking: vigil
- Every request is a vigil span (`vigil-telemetry`, tag 0.2.0), carrying:
  - the tool name and the hash of its arguments;
  - the handle;
  - cache hit or miss per layer (daemon, disk, cold), and the tier answered;
  - latency, peak memory, response bytes and an estimated token count.
- The audit feature is optional, for a tamper-evident request log.
- **Replay:** `stepv bench --replay <requests.jsonl>` re-runs a recorded agent session against the current build. It reports latency per tool, cache hit rate and response size, and feeds #83's budget file and ratchet, so real agent traffic becomes the benchmark.

## Scope
**P0** (works on today's OCCT path; no graph core needed):
- `stepv q` with: open/handle, summary, tree/children/find, `query` (jaq over the topology/product JSON), and `measure` (OCCT exact; bounding box, volume, area, centroid).
- `render` (offscreen viewer: fixed views, isolate/highlight, PNG).
- The daemon with an LRU memory budget and the on-disk result cache.
- vigil request spans.
- `stepv mcp` exposing the same tools.

**P1:**
- `relations`: containment, interference, minimum distance; section views.
- Replay benchmarking wired into #83.
- Answer tiers once #115's graph lands: bounding boxes and culling in milliseconds, before OCCT.

**P2:**
- `export` of subsets.
- A "spike" mode (agent-proposed edits evaluated as what-if transforms, read-only).
- A small set of composite questions (e.g. "fasteners and their holes").

## Pitfalls
- **Untrusted text in the file:** STEP names and descriptions are attacker-controlled strings. Returning them to a model is a **prompt-injection channel**. Mark every string as data (a quoted field, never interpolated into prose), cap its length, and say so in the tool docs.
- **Instancing:** a part used 40 times. Volume per definition vs per instance, and placements, must be explicit, or an agent double counts or under counts.
- **Undefined geometry:** open shells, non-manifold solids and failed heals have no volume. Error with a reason (the honesty contract).
- **Units and tolerance:** STEP declares units per context. Normalise, and always return the unit.
- **Cache correctness:** the key must include the kernel version and the heal/mesh options. A file edited in place gets a new content hash; never key on the path or mtime.
- **Daemon lifecycle:** stale sockets, two daemons racing, a crash taking every client down. One kernel per request stays inside the sandbox and the memory cap. The daemon itself is never the process that runs OCCT.
- **Output size:** trees and query results can be enormous. Paginate; cap bytes; summarise before listing.
- **Inline images** cost a lot of context. The default is a file path, inline only on request and under a cap.
- **Query language scope creep:** jaq (jq) and nothing else. A missing capability becomes a tool, not a new syntax.
- **mcpx overlap:** decide the cache boundary once (above). Two caches with different keys is the failure mode.

## Acceptance (P0)
- An agent session runs entirely through `stepv mcp`: it answers "how many distinct parts, the total volume of subassembly X, and a view isolating part Y" on a corpus file **without the file's text entering context**. Recorded as a replayable request log.
- The second identical call is served from cache: the vigil span shows the hit, and a test asserts its latency is far below a cold call.
- Every numeric answer carries its unit, tier and tolerance. An open-shell volume returns an error with its reason (a fixture test).
- STEP name strings are returned as escaped data fields, covered by a test with an injection-style name.
- Every kernel call runs under `stepv-cap` (#83) and the sandbox (#23).

## References
- #115 (graph model, tiers, honesty); #83 (caps, budgets, ratchet); #108 (admission); #23 (sandbox); #26 (topology JSON).
- vig-os/vigil (`vigil-telemetry` 0.2.0); gerchowl/mcpx (call cache, handles, vigil audit); the `jaq` crate.

## Next step
A spike review from several angles before any code: an agent-tooling/MCP designer, a CAD/geometry domain reviewer, a cache and systems reviewer, and a security reviewer for injection and sandboxing. Then an ADR for the tool surface and the cache boundary. Implementation is sequenced **after #83** and alongside or after #115's graph core.

---

# [Comment #1]() by [gerchowl]()

_Posted on November 8, 2026 at 07:58 PM_

## Owner input (2026-11-08): screening file text for prompt injection, and what text is worth exposing

**Text a STEP file carries** (all attacker-controlled):
- product names and descriptions;
- `PRODUCT_DEFINITION` ids;
- AP242 properties: materials, density, colour, layers;
- PMI and notes;
- `FILE_DESCRIPTION`, `FILE_NAME` author/organisation, comments.

Most of it is **useful semantics**: "M6x20 ISO 4762", "housing_lower", a material. An agent reasons far better with it, so hiding it all is the wrong default.

### Layered defence. Each layer reduces risk; none prevents injection alone, and the docs say so.
1. **Structural (always on):**
   - File text is only ever returned as typed data fields (`{"name": {"untrusted": "…"}}`), never interpolated into tool prose.
   - Length caps; Unicode normalisation (NFKC). Strip or flag bidi controls, zero-width and tag characters (U+E00xx). Flag homoglyph/mixed-script strings.
2. **Screening by a tool-less classifier.** Every string is scored by a model that **cannot generate text or call tools**, so a hostile string can't hijack it into doing anything. It only returns a probability.
   - Candidates: System One (`s1_decide`, calibrated p in tens of ms, already in this fleet) or an open prompt-injection classifier. Pick by measured recall/precision on a fixture set.
   - **Screen in combination, not just individually.** An attack can be split across sibling names ("ignore" / "previous" / "instructions"). So also score the concatenation per subtree and per answer *as it will be returned*: the unit that enters context is the unit that gets screened.
   - Verdicts are cached by (string hash, classifier version).
3. **Attestation ("signed clear").** Record each verdict in vigil's hash-chained, signed audit trail: string hash, classifier and version, p, decision. An answer carries `screened: {by, p_max, audit_ref}`, so a downstream agent or human can verify what was cleared and by what.
4. **Text policy per session:** `--text raw|screened|pseudonymous|off`.
   - **`screened` (default):** clean strings are passed through, flagged ones are replaced by a stable handle (`txt:3f2a…`) plus the reason.
   - **`pseudonymous`** ("rehash"): every string becomes a stable short id with only cheap derived features (length, script, a token class such as "looks like an ISO fastener designation"). For untrusted files where geometry alone suffices.
   - **`reveal(handle)`** returns the raw string. It is gated: allowed if screened clean, otherwise it needs an approval (gerchowl/mcpx's nodd approval flow fits here) and is logged.
5. **Inspection tools without exposure:**
   - `strings` lists every text field with counts, entropy, character classes and screening verdict, *without* the raw text unless it is cleared.
   - `query` (jaq) runs over names and attributes like any other field, so `[.. | .name? | select(test("^M[0-9]"))]` works. The output still passes through layers 1–4: jq selects, the policy decides what leaves.

### Pitfalls
- False positives on legitimate names (`DO_NOT_USE`, `IGNORE`) degrade usefulness. Measure them on the corpus, and keep flagged items revealable with approval rather than dropping them.
- Adaptive attackers can evade a fixed classifier. That's why layers 1 and 4 come first and screening comes second.
- The classifier version is part of the cache key. A model change re-screens.
- Combination screening multiplies calls. Bound it per answer (screen the final payload once) rather than over every subset.

### Acceptance (added to #124's P0/P1)
- **P0:** layer 1 + the `--text` policy (`raw`, `pseudonymous`) + `strings`, with an injection-fixture test file (a split-across-siblings attack, a bidi trick, tag characters).
- **P1:** the screening classifier + vigil attestation + `reveal` with approval, and recall/false-positive rates measured on the fixture set plus the corpus.


