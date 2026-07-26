# Package a local web app as a desktop application

The product will be a Tauri desktop application whose window and authorized Companion Devices use the same responsive React application. Tauri loads the packaged frontend in the desktop window; an embedded HTTP service makes the frontend and read-only documentation available to Companion Devices while Companion Mode is enabled. This gives the Desktop Host direct access to installed Mod Source while preserving a first-class same-network companion experience without uploads or a separate mobile application.

The shared frontend reads through one documentation-client interface with two thin adapters. The desktop adapter invokes Tauri commands, while the companion adapter uses HTTP. Both adapters use the same response shapes and call the same Rust application modules; transport handlers do not contain parallel product rules. Desktop-only capabilities remain in a separate Tauri adapter because a Companion Device cannot use them.

Companion access is explicitly enabled by the user and authorized through temporary QR-code sessions. This is a lightweight access gate for a read-only local service, not an account system or a claim of protection against interception by an attacker already controlling the local network.

Authorized companions may switch among Companion-Ready Caches without changing the desktop's active Target Mod. Missing, stale, or unverified documentation must be built on the Desktop Host; a Companion Device cannot trigger parsing, generation, asset conversion, or cache writes.

Companion Devices may read bounded Source Excerpts from a validated cache, but the service exposes only paths relative to the Mod Installation. Absolute host paths, arbitrary file access, and desktop-only actions such as opening an editor or revealing a file remain outside the companion API.

The embedded HTTP host, Tauri command adapters, parser, resolver, generator, and caches live in the same Tauri Rust process. The HTTP listener runs only while Companion Mode is enabled; normal desktop use does not require a loopback listener. CPU-intensive work runs through bounded background execution rather than on the UI or asynchronous I/O path. A sidecar is not part of the MVP.
