# ADR-002: Separate live captions from final transcript

## Status
Accepted

## Context
Live subtitles optimize for low latency, while final transcripts optimize for refinement, glossary correction, and possible speaker assignment.

## Decision
Model live captions and final transcript as separate domain artifacts with separate states and versioning.

## Consequences
### Positive
- Clear product semantics
- Easier UI expectations
- Supports reprocessing and transcript version comparison

### Trade-offs
- More storage and more domain objects
- Review UI must explain differences between live and final artifacts
