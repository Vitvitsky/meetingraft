# ADR-004: Audio capture — microphone + system audio via Core Audio process taps

## Status
Accepted

## Context
The product must record both sides of a meeting: the user's own voice and
remote participants in Zoom / Teams (desktop app or browser). Remote voices
exist only as system playback audio, so microphone capture alone is not
enough. The client is native-first (ADR-001), so the capture mechanism must
not require third-party drivers or manual audio routing.

## Options considered

1. **Core Audio process taps** (`AudioHardwareCreateProcessTap` /
   `CATapDescription`, macOS 14.2+). Captures playback audio of the whole
   system or of selected processes without touching the screen. Uses the
   dedicated "System Audio Recording" permission (macOS 15+), not Screen
   Recording. Works the same whether the meeting runs in the Zoom/Teams app
   or in a browser tab. This is the approach used by modern native meeting
   companions.
2. **ScreenCaptureKit audio.** Delivers system audio, but requires the
   Screen Recording permission (alarming for users) and drags a screen
   capture session along for an audio-only need.
3. **Virtual audio driver** (BlackHole / Loopback). Requires installing a
   driver and manually re-routing meeting output; fragile UX, contradicts
   native-first.
4. **Meeting bot that joins the call** (recall.ai style). Cloud-dependent,
   visible participant in the meeting, not native; out of scope for v1.

## Decision
Capture two synchronized PCM streams in the Swift platform layer:

- microphone via `AVAudioEngine` (user's voice);
- system playback via a Core Audio process tap (remote participants).

Keep the two streams **separate end-to-end** through the chunking pipeline,
local recording manifest, and STT: the mic stream is "me", the system
stream is "others". Mixing happens only where a consumer explicitly needs a
mix. Timestamp alignment of the two streams is owned by the Rust session
engine.

Minimum supported macOS: **15 (Sequoia)** — gives the dedicated system
audio permission prompt and a stable tap API.

## Consequences
### Positive
- No drivers, no manual routing; permissions are native prompts
  (microphone + system audio recording).
- Independent of which meeting app is used — captures any playback audio.
- Channel-level "me vs others" separation for free; post-call speaker
  assignment (ADR-002 refinement) starts from two clean channels instead
  of one mixed track.

### Trade-offs
- Raises the minimum OS to macOS 15.
- Two-stream capture doubles buffer bookkeeping; alignment logic must be
  tested in the session engine.
- System tap records *all* playback during a session (music,
  notifications); scoping to selected processes is a later refinement.
