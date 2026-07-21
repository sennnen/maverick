# Product

## Register

product

## Users

Privacy-conscious wearable owners using Maverick on their own iPhone or Android phone, often while
pairing, troubleshooting, or reviewing sensitive health data. They need clear device state and
honest metric availability without accounts, cloud dependencies, or protocol expertise.

## Product Purpose

Maverick is a local-first wearable-data platform. It installs independently signed device
connectors, acquires BLE data through native transports, performs decoding and analytics on-device
in one shared Rust core, and renders the same trustworthy result on iOS and Android. Success means a
user can add, approve, connect, update, roll back, or remove a connector without rebuilding the app
and can always understand what is installed, active, trusted, or unavailable.

## Brand Personality

Private, candid, precise. The experience should feel calm and technically credible while retaining
the Aura shell's vivid metric identity and editorial confidence. It never disguises provisional
science, missing data, or a failed trust decision as success.

## Anti-references

Not a vendor clone, generic admin dashboard, neon hacker console, card-grid marketplace, or
permission funnel that pressures users past inspection. Avoid fake health scores, decorative
security theatre, unexplained protocol jargon, hidden cloud assumptions, and device-specific UI
that makes third-party connectors feel second class.

## Design Principles

- Make trust legible before asking for approval.
- Keep device identity separate from connector provenance and version state.
- Prefer one progressive workflow over modal chains or duplicated import paths.
- Preserve native platform conventions while keeping the information model byte-for-byte aligned.
- Treat empty, loading, revoked, failed, and rolled-back states as first-class product states.

## Accessibility & Inclusion

Use native Dynamic Type/font scaling, screen-reader labels and state announcements, minimum touch
targets, high-contrast semantic states that do not rely on color alone, and reduced-motion behavior.
Copy must remain understandable without BLE, cryptography, or connector-development knowledge.
