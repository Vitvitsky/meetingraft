# ADR-005: Live STT — on-device Whisper behind a Rust `SttEngine` trait

## Status
Accepted

## Context
Live captions need Russian-first quality with mixed English and Spanish
(ADR-003), acceptable latency for subtitles (~1–2 s), and they process
work-meeting audio — privacy matters. ADR-004 provides two PCM streams
(mic + system). The choice here decides whether the backend is on the
critical path for Stage 1.

## Options considered

1. **On-device Whisper** — `whisper.cpp` via the `whisper-rs` bindings in
   the Rust core, model `large-v3-turbo` (quantized) with Metal on Apple
   Silicon; VAD-driven segmentation (Silero VAD) over a sliding window.
   Strong ru/en/es including code-switching; audio never leaves the
   machine; no backend needed for Stage 1.
2. **Apple Speech (`SpeechAnalyzer` / `SFSpeechRecognizer`).** Native and
   fast, but Russian on-device quality/availability is the open risk;
   would also pull STT into the Swift layer, against the "domain logic in
   Rust" rule.
3. **Cloud streaming STT** (Yandex SpeechKit — best-in-class ru; Deepgram /
   OpenAI Realtime — best streaming ergonomics). Higher potential quality,
   but meeting audio leaves the machine, per-minute cost, and a streaming
   gateway becomes a Stage 1 dependency.

## Decision
For v1 live captions: **on-device Whisper (`whisper.cpp` / `whisper-rs`,
`large-v3-turbo` quantized, Metal)** inside the Rust core, fed by the
chunking pipeline, segmented by Silero VAD.

The engine sits behind a Rust trait (`SttEngine`) that accepts the language
policy (`primary_language`, `allowed_languages`) and emits partial/final
caption events — so a cloud provider can be swapped in later without
touching Swift or the session engine. Glossary bias is applied through the
engine interface (initial prompt / token bias), keeping ADR-003's
Russian-first routing.

Post-call refinement is *not* bound by this decision: it may use a larger
model, cloud STT, or a home-server worker — fixed later in ADR-007 scope.

## Consequences
### Positive
- Backend is off the critical path for all of Stage 1 (roadmap Phases 1–4);
  ADR-007 can be deferred until Phase 6.
- Privacy by default: raw meeting audio stays local.
- One STT engine handles ru/en/es and code-switching; language hints map
  directly onto Whisper decoding.

### Trade-offs
- Requires Apple Silicon and enough RAM for the model (~1.5 GB quantized);
  Intel Macs are out of scope.
- Whisper is not a true streaming model: latency is bounded by VAD window
  tuning (target ≤ 2 s for partials), to be validated in Phase 4 against
  the latency budget.
- Local model files must be downloaded/managed by the app (first-run
  download flow). Official ggml weights come from Hugging Face
  (`ggerganov/whisper.cpp`); see `apps/macos/Scripts/download-stt-model.sh`.
- Whisper may emit credit-style hallucinations on silence/noise (e.g. Russian
  «авторы субтитров…»). Live engine filters known markers and drops high
  `no_speech` segments; Silero VAD remains the longer-term fix.

## Amendment 2026-08-27 — what actually shipped instead of Silero VAD

This ADR names Silero VAD twice: once in the decision («segmented by
Silero VAD») and once as the long-term fix for Whisper's hallucinations.
**Neither happened.** What the live path runs is `stt::NoiseGate` — frame
RMS against an adaptive estimate of the room floor, speech being anything
that exceeds it threefold.

The substitution was not a shortcut taken quietly; it was measured twice
(Epic 18) and improved both times. But the two things are not the same
kind of thing, and the difference matters where it decides:

- a gate tells **louder than the room** from **not louder**. A knock on
  the desk, a cough and music all pass it;
- a VAD tells **speech** from **not speech**, and pays for that with
  latency — it needs a window before it can decide.

Numbers the gate stands on today (Mac, 2026-08-11): 26 model runs per
minute on a silent room, down from 48, and 32.1 per minute on a real
22-minute meeting.

Silero now exists in the repository as a **second column in `gate-probe`**,
not in the live path (PR #172). It costs a 0.6 MB model and no new
dependency: sherpa-onnx is already linked for voice separation and
carries `VoiceActivityDetector` inside.

First numbers (Linux, synthetic room plus studio speech): on deliberate
non-speech both give zero; on speech starting three seconds in they agree
on 92.5% of frames, and Silero **did not clip the onset** — both opened
on the same frame. That was its main expected cost.

What decides whether the live path changes is not in these numbers:
meetings instead of synthetic material, watts instead of run counts, and
the VAD's own per-frame cost. Design:
`docs/superpowers/specs/2026-08-27-silero-vad-vs-noise-gate-design.md`.

The decision of this ADR is unchanged — the engine still sits behind
`SttEngine` and segmentation still belongs to the Rust core. Only the
statement «segmented by Silero VAD» is, for now, a plan rather than a
description.
