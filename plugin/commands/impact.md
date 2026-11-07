---
description: Show what may break if a symbol changes (callers, subtypes, tests to run)
argument-hint: <symbol, e.g. Store::save>
---

Use the adaptive-codegraph `impact` tool on `$ARGUMENTS` (depth 4). Then summarize for me:

1. The direct callers and implementations that must be checked or updated.
2. Anything reached only through an interface or base class (may be affected).
3. The tests to run.

If the symbol name is ambiguous, show the candidates and ask which one I mean.
