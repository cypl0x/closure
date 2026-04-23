# Golden org corpus

Every file in this directory is a roundtrip fixture. The invariant
(see `docs/spec.md` I1) is that `closure_org::print(closure_org::parse(src)?) == src`
byte-for-byte.

## Rules

- One construct per file, named descriptively (`heading-basic.org`,
  `drawer-properties.org`, ...).
- No trailing-newline normalization. If the file ends without a newline,
  that's the fixture.
- Files are hand-written; keep them minimal. If you add a file, also add
  it to the roundtrip test corpus at the same commit.
- UTF-8 only.

## Organization

Files are named `<category>-<variant>.org`. Categories used:

- `empty` — empty or whitespace-only files
- `paragraph` — plain text
- `comment` — `#` comment lines
- `keyword` — `#+KEY: value` lines
- `heading` — `* headline` constructs
- `drawer` — `:PROPERTIES:` / `:LOGBOOK:` / custom drawers
- `list` — `-`, `+`, `1.` lists, checkboxes
- `block` — `#+BEGIN_SRC ... #+END_SRC`, `QUOTE`, `EXAMPLE`, ...
- `table` — pipe tables
- `timestamp` — active/inactive timestamps and ranges
- `link` — `[[target][desc]]`
- `markup` — inline `*bold*`, `/italic/`, etc.

## How edge-cases land here

When a bug is found on real content, the minimal reproducer becomes a
new fixture in this directory with a name describing the bug. This file
stays part of the corpus forever.
