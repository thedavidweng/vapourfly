# GUI information architecture and design system

The Vapourfly desktop GUI is a GPUI + [gpui-component](https://github.com/longbridge/gpui-component) application. Light desktop mockups are the default source of truth for chrome and navigation, with a runtime switch to the dark design-system shell. Light mode uses a warm near-white canvas, white bordered cards, and a restrained orchid accent. Dark mode uses a cool surface hierarchy and violet brand accent. Both modes share the same structural tokens, components, hierarchy, and layout regions; only palette values change. Desktop top chrome shows breadcrumb and metrics; native OS window controls remain authoritative — the app does not draw its own traffic-light circles. Monochrome line icons are common. Theme mode is toggled from the top chrome and Settings → Appearance, and persists across launches in a GUI-only file (`gui-theme` under the platform data directory), not domain config.

Top-level destinations are Discover, Library, Recommendations, Playlists, Collections, Data Sources, and Settings. Library is the default landing view. Discover is a first-class destination. Junk is a Library workflow with mode, preview, collection, and hide actions. Backups and restore live under Settings. Domain concepts and safety behavior remain unchanged: Playlist and Steam Collection stay distinct, every Steam write stays confirmation-gated, and hydration follows ADR-0009.

## Amendment 2026-08-23: component adoption, motion budget, and feedback split

A component-adoption pass replaced the remaining hand-drawn controls with
gpui-component semantics. App code now colors only through `cx.theme()`
tokens seeded from the ADR palettes (the sanctioned exception is the
editorial `tag_tint`/`tint` artwork palette); layout uses rem utilities.
Commands are modeled once as GPUI actions: the native macOS menu bar,
keybindings (`cmd-1..7` navigate, `cmd-r` refresh, `cmd-f` search,
`cmd-s` save playlist, `cmd-j` junk, `cmd-b` sidebar, `cmd-shift-t`
theme), and in-window controls dispatch the same handlers on the shell
root. Sort and Proton-tier filters are Selects reconciled against app
state each frame; secondary filters live under a "More filters" dropdown;
row commands are icon buttons mirrored by right-click context menus via
payload-carrying actions.

Motion budget: purposeful minimum — opacity-first fades of 150–250 ms
ease-out for dialogs, banners, and the top-3 recommendation cards; no
entrance staggering, no springs (the 0.5.x library has none). There is no
OS reduce-motion query available to gpui 0.2.2, so a persisted
"Reduce motion" preference lives beside the theme file
(`gui-reduce-motion`) with a Settings → Appearance switch; when enabled,
every animation helper returns its element unchanged.

Feedback split: destructive or Steam-writing flows confirm through
`WindowExt` dialogs opened from a render-safe deferred pump — titles name
the object ("Sync playlist “x”?"), bodies carry only consequence/recovery
information, and the demo-mode write gate is enforced both on the button
(disabled state) and inside the handler (re-check). Sticky operational
errors stay as inline dismissible banners tied to the view; locally
triggered transient successes (copy share code, settings saved, copied
app id) surface as notifications instead.
