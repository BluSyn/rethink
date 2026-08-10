# Long-term approach: understanding non-TLV UART (UartBinary)

## Principle

Every appliance→cloud byte stream is potentially meaningful. Climate **TLV** is only one dialect. Other UART kinds use the same envelope but different body layouts. We refuse to invent false TLV tags, but we **always attempt** a structured reading:

1. Parse envelope (kind, b5–b7, len, CRC).
2. Classify dialect (climate TLV vs binary).
3. For binary: dump, stats, **heuristics** (temp/humidity/counter patterns).
4. Prefer **paired captures** + cloud/app correlation to confirm fields.
5. When a field is solid, promote it to a named decoder and (if useful) an HA entity.

## Tooling

- `rethink_util::uart_binary` — analysis + RE export text
- Management `POST /api/decode` → `protocol: UartBinary`, `binaryAnalysis: {…}`
- `POST /api/re/export` → full “UART binary RE export” text (text breakdown / LLM paste)

## Experiments that work

| Experiment | What it teaches |
|------------|-----------------|
| Same kind, before/after one app toggle | Which body offsets change |
| Bridge + cloud notification | LG’s labelled decode of the same event |
| Filter reset / long run | Counter-like fields (hours, residual) |
| Cross-model same kind | Shared private layouts vs model-specific |

## Success criteria for a “decoded” binary family

- Stable `(kind, b5, b6)` identity
- Documented offset/type/unit for ≥1 useful field
- Diff test or capture fixture in-repo
- Optional HA entity only if user-facing value exists
