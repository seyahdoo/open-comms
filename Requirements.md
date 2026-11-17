# Requirements
- Build a web client app usable on Windows, macOS, Linux, Android and iOS through modern browsers.
- Build server app for linux.
- Have support for text channels and voice channels.
- Voice channels should support server relayed voice communications.
- Voice channels should support screen, window and audio sharing.
- Text channels should support basic text messaging.
- Text channels should support message replies and those replies should also be repliable. And all of these replies should be hidden inside the message.
- Text channels should support emoji responses.
- Server should use mongodb as database.
- Server app should be able to deploy with simple docker compose file.
- Server app should be written in rust.
- Client app should be written as a web app (HTML/CSS/TypeScript) and served by the server.

## Product decisions
- No guild/workspace support (single-server channel model).
- Permissions are fully permissive for now; add authorization/roles later.
- No end-to-end encryption; rely on in-transit encryption (TLS/WSS, DTLS-SRTP for WebRTC) for MITM protection.
- Media uses WebRTC with server-side relay/SFU behavior.
- Mobile targets (Android and iOS) are first-class with best-effort full feature parity; platform-specific limitations (e.g. window/system-audio capture semantics) should be handled as close to desktop parity as possible.
- Web-first client: one TypeScript web app shared across all platforms; browser WebRTC APIs (`getUserMedia`, `getDisplayMedia`, `RTCPeerConnection`) are used for media. Optional native wrappers may be added later for packaging/permissions, but are not required for the core client.

