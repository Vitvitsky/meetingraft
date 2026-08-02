# ADR-003: Speech recognition languages (ru primary)

## Status
Accepted

## Context
Meetings are expected primarily in Russian, with frequent English terms and occasional Spanish.
Recognition quality for Russian must not be sacrificed for multilingual convenience.
Live STT and post-call refinement must share one language policy so captions and final transcript stay aligned.

## Decision
Support exactly three speech languages for v1:
- `ru` — primary and default session language;
- `en` — supported;
- `es` — supported.

Session and processing contracts carry:
- `primary_language` (default `ru`);
- `allowed_languages` (default `{ru, en, es}`).

STT providers and post-call jobs must honor this policy. Russian-first model routing / prompts / glossary bias when trade-offs appear.

## Consequences
### Positive
- Clear product default for Russian-speaking users
- Explicit multilingual scope without open-ended language matrix
- Same policy for live captions and final transcript

### Trade-offs
- Other languages are out of scope until a new ADR
- Mixed-language accuracy depends on provider capabilities within the allowed set
