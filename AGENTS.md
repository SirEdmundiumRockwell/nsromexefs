# Project Instructions

## Codebase Memory

- Use `codebase-memory-mcp` for structural discovery: symbols, callers, impact analysis, architecture, and dead-code candidates.
- Fall back to `grep`/`glob` for literals, error messages, configuration files, and macro-generated code.
- After code changes, re-index with `index_repository` and verify `index_status`; if the generation predates the change, re-index before relying on graph results.
- Before negative or exhaustive claims, call `check_index_coverage`. A clean result is best-effort, not proof of completeness.
- Do not commit the graph database or generated index artifacts.
