# Product

## Register

product

## Users

Linux desktop users who want their current media playback reflected in Discord. They use mprisence from a terminal, expect common players to work without configuration, and need setup and diagnostics to stay understandable across package managers and desktop environments.

## Product Purpose

mprisence turns MPRIS metadata from local and web media players into Discord Rich Presence. Success means a user can install it, confirm it sees their player, keep it running across logins, and recover from setup problems without learning the implementation details first.

## Brand Personality

Direct, capable, and quiet. The product should feel like a dependable Linux utility: concise when things work, specific when they do not, and never decorative at the expense of clarity.

## Anti-references

Avoid wizard-heavy setup, dashboard-style decoration in terminal output, unexplained systemd jargon, hidden filesystem mutations, and commands whose names obscure whether they start, stop, install, or remove anything.

## Design Principles

- Make the common path one obvious command.
- Report the resulting state, not just the action attempted.
- Keep every setup action reversible and explain what was changed.
- Preserve user-managed configuration and fail safely on conflicts.
- Put detailed diagnostics behind explicit status and doctor commands.

## Accessibility & Inclusion

Terminal output must remain understandable without color, symbols, animation, or a wide viewport. Status and failures should use plain language, stable labels, and actionable next steps. Commands should work without root privileges for normal per-user setup.

