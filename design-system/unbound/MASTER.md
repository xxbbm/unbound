# Unbound · iOS light desktop

Reference: user-supplied IMG_0209.JPG. Product: a native Rust/egui iPhone management application. This is the controlling visual brief.

## Direction

Silver-white shell, subtle translucent surfaces, blue selection and quiet elevation. The defining element is the U ribbon mark; the rest of the interface remains restrained. Apply the reference to the existing four functional sections, without inventing accounts, automation, device groups or sample devices.

```text
U  Unbound                                Search       Help
┌────────────────┐ ┌───────────────────────┐ ┌──────────────────┐
│ Device overview│ │ Device operations     │ │ Model image      │
│ Inspection     │ │ Search + live row     │ │ Connection state │
│ Backup/restore │ │ Actual USB count      │ │ Read parameters  │
│ Apps/files     │ │ Five action shortcuts │ │ Full information │
│ Help / About   │ │                       │ │                  │
└────────────────┘ └───────────────────────┘ └──────────────────┘
                         Running task / stop / status
```

## Tokens

| Role | Value |
|---|---|
| Background | #F2F4F8 |
| Primary surface | White, 85–92% opacity |
| Selected surface | #E6EEFF |
| Primary action / focus | #2863E4 |
| Main text | #1C222E |
| Secondary text | #60697A |
| Boundary | #DCE2EC |
| Connected indicator | #2FB062, always accompanied by text |

Chinese typography uses the platform Chinese font (Microsoft YaHei / PingFang), with local egui fallbacks. No remote font fetches. Title 26–30 pt, body 16 pt, actions 15 pt, secondary labels 13–14 pt. Use natural case, normal text spacing and consistent left alignment.

Main surfaces have 22 pt corners and one restrained elevation level. Controls use 12–16 pt corners; selected rows use 12 pt. Spacing follows 8/12/16/20/24 pt increments. Decorative icons are vector strokes in one visual family. Buttons retain native egui click and keyboard behavior.

## Reference review

UI/UX Pro Max's initial system-wide query returned a marketing-style hero pattern, so that pattern and its suggested web fonts were discarded. A narrower style search returned Glassmorphism, matching the supplied reference. Apply that style with high surface opacity so content stays readable. Use the skill's desktop keyboard-focus and state-feedback guidance; no unsupported egui stack guidance was invented.

Translucent egui layers and desaturated background gradients create the frosted appearance; there is no screenshot-based backdrop blur and no expensive per-frame image filter. Data tables and forms remain crisp. Motion is limited to the native hover/focus response, without automatic layout animation.

## Interaction contract

- Global search finds existing actions and the connected device; selection opens the real page/form.
- Device search filters the actual USB row. Never fill the list with the reference's example phones.
- Action shortcuts open the existing operation forms. Erase/restart do not execute directly.
- Copy controls copy the complete device value; shortened identifiers remain available via hover.
- Empty state explains how to connect and links to working help.
- Below 1180 pt window width, stack device details below the overview so forms remain readable.
- Preserve the three-column actual-readings report. No factory-value comparisons or authenticity badges.

## Delivery checks

Inspect the native window at normal and compact desktop sizes, review long values and empty state, verify search routing/help and keyboard focus, and ensure asynchronous USB polling does not become a foreground task. Do not execute write operations during visual verification.

## Brand mark update

The U now has a shorter cyan left arm and a taller violet right arm. The two ribbons overlap across the lower middle with a small folded edge. One vector contour set produces assets/brand/unbound.svg, the embedded PNG and the multi-size Windows ICO. Header, native window/taskbar and Windows executable resources use the same mark.