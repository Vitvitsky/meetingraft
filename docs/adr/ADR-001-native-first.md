# ADR-001: Native-first macOS application

## Status
Accepted

## Context
The product needs live subtitles during meetings, native macOS interaction patterns, local audio capture, and a polished desktop experience.

## Decision
Build the client as a macOS-first app using SwiftUI and AVFoundation.
Use Rust for performance-sensitive and reusable domain logic.

## Consequences
### Positive
- Native UX aligned with macOS expectations
- Direct access to platform audio APIs
- Clean separation between UI shell and core logic

### Trade-offs
- More build/tooling complexity than a pure single-language app
- FFI boundary must be designed carefully
