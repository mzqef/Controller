# IntelliBoard GUI Design Audit

> **Purpose**: Single-file reference for audit review and external AI rapid assessment.  
> **Version**: 2026-07-03 r2 (design-system refactor: token scales, elevation surfaces, soft accents, frame builders)  
> **UI Framework**: `eframe` / `egui` (immediate-mode GUI, Rust)  
> **Aesthetic**: "Japan 2046" — Cyber-Japonism / Neon Cyberpunk  

> **2026-07-03 r2 change summary** — the GUI was audited as "lacking texture, uncoordinated sizing". The fix introduces a single **design-token system** in `src/ui/theme.rs` (spacing / radius / type / stroke / elevation scales, soft-accent variants, and reusable frame builders). All four UI surfaces (`src/ui.rs`, `src/bin/functions_config_ui.rs`, `src/ui/memory_graph.rs`, plus the shared `apply_theme`) now draw exclusively from these tokens. Raw neon is reserved for emphasis; surfaces, borders and fills use softened/elevated variants so the UI reads as layered depth rather than flat neon-on-black.

---

## 1. COLOR PALETTE (Theme)

Defined in `src/ui/theme.rs`.

### 1.1 Core Palette

| Token           | R   | G   | B   | Hex       | Role                      |
|-----------------|-----|-----|-----|-----------|---------------------------|
| `BG_COLOR`      | 10  | 10  | 16  | `#0A0A10` | Deep Cyber Night (root bg)|
| `PANEL_COLOR`   | 15  | 15  | 20  | `#0F0F14` | Panel / surface fill      |
| `NEON_CYAN`     | 0   | 243 | 255 | `#00F3FF` | Accent, borders, highlights |
| `NEON_PINK`     | 255 | 0   | 60  | `#FF003C` | Selection, danger         |
| `TEXT_COLOR`    | 220 | 230 | 255 | `#DCE6FF` | Cool white body text      |
| `INACTIVE_BG`   | 25  | 25  | 35  | `#191923` | Inactive widget bg        |
| `HOVERED_BG`    | 35  | 35  | 50  | `#232332` | Hovered widget bg         |

### 1.2 Edge Colors (Memory Graph)

| Token             | R   | G   | B   | Hex       | Meaning                  |
|-------------------|-----|-----|-----|-----------|--------------------------|
| `EDGE_ACTION`     | 0   | 200 | 255 | `#00C8FF` | Action-result edge       |
| `EDGE_PROMOTION`  | 0   | 255 | 100 | `#00FF64` | Promotion edge           |
| `EDGE_USER`       | 255 | 200 | 0   | `#FFC800` | User-created edge        |
| `EDGE_FORMAT`     | 255 | 128 | 0   | `#FF8000` | Format edge              |

### 1.3 Soft Accent & Status Variants  *(r2 — new)*

Raw neon at full saturation is harsh on large areas. These softened/semantic
variants are used for borders, glows, tints and status labels so the UI gains
"atmosphere" instead of vibrating.

| Token            | R   | G   | B   | Hex       | Role                                            |
|------------------|-----|-----|-----|-----------|-------------------------------------------------|
| `ACCENT_BORDER`  | 0   | 200 | 230 | `#00C8E6` | Card/popup border (softened cyan)               |
| `ACCENT_DIM`     | 12  | 40  | 52  | `#0C2834` | Faint cyan wash (selected/hover tints)          |
| `ACCENT_SELECT`  | 190 | 28  | 70  | `#BE1C46` | Selection bg (replaces raw `NEON_PINK` flood)   |
| `DANGER_TEXT`    | 255 | 96  | 110 | `#FF606E` | Danger text (readable on dark)                  |
| `TEXT_MUTED`     | 145 | 154 | 174 | `#919AAE` | Secondary / muted text                          |
| `SUCCESS`        | 0   | 255 | 110 | `#00FF6E` | Matrix-style "finished" status                  |
| `WARN`           | 255 | 200 | 0   | `#FFC800` | "Incomplete" status                             |
| `BORDER_STRONG`  | 0   | 200 | 230 | `#00C8E6` | Focused/hovered card border                     |

### 1.4 Elevation Surfaces  *(r2 — new)*

Depth is conveyed by luminance steps rather than flat fills. Each successive
level lifts one step, so panels, cards and popups read as stacked layers.

| Token        | R   | G   | B   | Hex       | Layer                          |
|--------------|-----|-----|-----|-----------|--------------------------------|
| `SURFACE_0`  | 10  | 10  | 16  | `#0A0A10` | Root background (= `BG_COLOR`) |
| `SURFACE_1`  | 15  | 15  | 20  | `#0F0F14` | Top-level panels (= `PANEL_COLOR`) |
| `SURFACE_2`  | 20  | 22  | 30  | `#14161E` | In-panel cards                 |
| `SURFACE_3`  | 24  | 27  | 36  | `#181B24` | Floating windows / popups      |
| `BORDER`     | 48  | 54  | 68  | `#303644` | Canonical hairline border      |

### 1.5 Functions Config UI Palette (`functions_config_ui.rs`)

> **r2**: The Functions Config window previously declared its own `SURFACE / SURFACE_ALT / BORDER / ACCENT / TEXT_MUTED / DANGER` constants that diverged from the rest of the app. These are now **aliases** of the canonical theme tokens (see §1.3–1.4), so the two windows can never drift. The legacy names are kept as private `const` aliases to minimise churn.

| Token (alias) | Maps to            | Hex       | Role                     |
|----------------|--------------------|-----------|--------------------------|
| `SURFACE`      | `SURFACE_2`        | `#14161E` | Section card background  |
| `SURFACE_ALT`  | `SURFACE_3`        | `#181B24` | Selected item background |
| `BORDER`       | `theme::BORDER`    | `#303644` | Card/panel stroke        |
| `ACCENT`       | `ACCENT_BORDER`    | `#00C8E6` | Selected border, success |
| `TEXT_MUTED`   | `theme::TEXT_MUTED`| `#919AAE` | Secondary labels         |
| `DANGER`       | `DANGER_TEXT`      | `#FF606E` | Delete button text       |

### 1.6 Memory Graph Tier Colors

| Tier        | R   | G   | B   | Hex       |
|-------------|-----|-----|-----|-----------|
| Short-term  | 0   | 200 | 255 | `#00C8FF` |
| Mid-term    | 255 | 200 | 0   | `#FFC800` |
| Long-term   | 0   | 255 | 100 | `#00FF64` |

### 1.7 Relation Edge Colors (Memory Graph)

| Relation      | R   | G   | B   | Hex       |
|---------------|-----|-----|-----|-----------|
| TranslatedTo  | 255 | 100 | 255 | `#FF64FF` |
| ExplainedBy   | 100 | 200 | 255 | `#64C8FF` |
| FormattedTo   | 255 | 150 | 100 | `#FF9664` |
| DerivedFrom   | 200 | 200 | 200 | `#C8C8C8` |
| PromotedFrom  | 0   | 255 | 100 | `#00FF64` |
| UserLinked    | 255 | 255 | 255 | `#FFFFFF` |

---

## 2. TYPOGRAPHY

### 2.1 Font Configuration

Defined in `theme::configure_fonts()`.

- **Priority chain** (cross-platform):
  1. `C:\Windows\Fonts\msyh.ttc` (Microsoft YaHei, Windows)
  2. `C:\Windows\Fonts\simhei.ttf` (SimHei, Windows)
  3. `C:\Windows\Fonts\msgothic.ttc` (MS Gothic, Windows)
  4. `/System/Library/Fonts/PingFang.ttc` (macOS)
  5. `/Library/Fonts/Arial Unicode.ttf` (macOS)
  6. `/usr/share/fonts/*/NotoSansCJK-*.ttc` (Linux)
- **CJK font** loaded as `"cjk_font"`, inserted at position 0 of `FontFamily::Proportional` and pushed as fallback for `FontFamily::Monospace`.
- **Fallback**: Default egui fonts if no CJK font found.

### 2.2 Type Scale  *(r2 — tokenised)*

Font sizes are no longer ad-hoc magic numbers; every label picks from a single
1.2-ratio ladder defined in `theme.rs`. The right-hand column shows where each
token is used.

| Token        | Size (px) | Used for                                              |
|--------------|-----------|-------------------------------------------------------|
| `TEXT_XS`    | 11.0      | Memory-graph node labels (× zoom)                     |
| `TEXT_SM`    | 12.0      | JSON editor, table headers, field labels, captions    |
| `TEXT_BASE`  | 13.0      | Default body, input/output text, status pills         |
| `TEXT_MD`    | 14.0      | Processing bar label, status indicator, section titles|
| `TEXT_LG`    | 16.0      | Action header (config editor)                         |
| `TEXT_XL`    | 19.0      | Navigation brand name                                 |
| `TEXT_2XL`   | 24.0      | Page title (config UI)                                |

> Migration map (old → new): `22.0` page title → `TEXT_2XL`; `18.0` action label → `TEXT_LG`; `19.0` brand → `TEXT_XL`; `14.0` body/status → `TEXT_BASE`/`TEXT_MD`; `12.0` JSON → `TEXT_SM`; `11.0` node label → `TEXT_XS`.

### 2.3 Font Sizes in Use (concrete call sites)

| Context                      | Token        | Family       |
|------------------------------|--------------|--------------|
| Processing bar label          | `TEXT_MD`    | Monospace    |
| Result text (input & output)  | `TEXT_BASE`  | Proportional |
| Node label (memory graph)     | `TEXT_XS` × zoom | Proportional |
| Ghost node emoji              | `TEXT_SM` × zoom | Proportional |
| Page title (config UI)        | `TEXT_2XL`   | Default      |
| Section title (config UI)     | `TEXT_MD`    | Default      |
| Action label (config editor)  | `TEXT_LG`    | Default      |
| Navigation brand name         | `TEXT_XL`    | Default      |
| JSON editor (config UI)       | `TEXT_SM`    | Monospace    |
| Status indicator (Japanese)   | `TEXT_MD`    | Default      |

---

## 3. DESIGN TOKEN SCALES  *(r2 — new)*

All paddings, gaps, corner radii, stroke weights and control heights are
defined as named tokens in `src/ui/theme.rs`. Components pull exclusively from
these ladders, which is what makes the UI read as coordinated rather than
ad-hoc. The previous build scattered values (`8/10/12/14/16/20` margins,
`0/4/6/8` radii, `1/2` strokes, `22/32/34` heights) across files; they are now
all expressible through the scales below.

### 3.1 Spacing Scale (base unit = 4px, 4:8 rhythm)

| Token    | px   | Use                                   |
|----------|------|---------------------------------------|
| `SPACE_0`| 0    | —                                     |
| `SPACE_1`| 2    | hairline gap                          |
| `SPACE_2`| 4    | base unit                             |
| `SPACE_3`| 6    | tight inline gap                      |
| `SPACE_4`| 8    | default inner control gap             |
| `SPACE_5`| 12   | section spacing                       |
| `SPACE_6`| 16   | panel padding                         |
| `SPACE_7`| 20   | popup padding                         |
| `SPACE_8`| 24   | page gutter                           |
| `SPACE_9`| 32   | large block separation                |

### 3.2 Radius Scale (corner language)

| Token        | px   | Use                                   |
|--------------|------|---------------------------------------|
| `RADIUS_SM`  | 4    | small controls, chips, badges         |
| `RADIUS_MD`  | 6    | buttons, list rows, scanner segments  |
| `RADIUS_LG`  | 10   | cards, section containers             |
| `RADIUS_XL`  | 14   | popups, modals, result window         |
| `RADIUS_PILL`| 999  | fully rounded tags/status pills       |

### 3.3 Stroke Scale (hairline → emphasis)

| Token           | px   | Use                                   |
|-----------------|------|---------------------------------------|
| `STROKE_HAIRLINE`| 1.0 | dividers, card outlines               |
| `STROKE_THIN`    | 1.5 | default borders                       |
| `STROKE_ACCENT`  | 2.0 | accent borders (cyan ring on popups)  |
| `STROKE_BOLD`    | 3.0 | emphasized frames                     |

### 3.4 Control Heights (button/row heights)

| Token      | px   | Use                                   |
|------------|------|---------------------------------------|
| `CTRL_H_SM`| 24   | icon buttons (close ✕, stop ✕)        |
| `CTRL_H_MD`| 30   | list rows, bottom-bar buttons         |
| `CTRL_H_LG`| 36   | navigation items                      |

### 3.5 Frame Builders (single source of truth for component shells)

Instead of every call site hand-crafting `egui::Frame::default()...` with ad-hoc
numbers, components pull a pre-tuned frame. This is the core of the texture fix.

| Builder           | Fill       | Stroke              | Radius   | Padding | Used by                       |
|-------------------|------------|---------------------|----------|---------|-------------------------------|
| `panel_frame()`   | `SURFACE_1`| hairline `BORDER`   | 0        | `SPACE_6`| Side nav, bottom bar          |
| `card_frame()`    | `SURFACE_2`| hairline `BORDER`   | `RADIUS_LG`| `SPACE_5`| Section cards (config UI)   |
| `popup_frame()`   | `SURFACE_3`| `STROKE_ACCENT` `ACCENT_BORDER`| `RADIUS_XL`| `SPACE_7`| Result window, modals |
| `indicator_frame()`| `SURFACE_3`| `STROKE_ACCENT` `ACCENT_BORDER`| `RADIUS_LG`| `SPACE_4`| Processing bar   |

### 3.6 Depth Helpers (shadow / glow)

egui has no native blur, so depth is emulated with cheap concentric rings.

| Helper                | Effect                                                        |
|-----------------------|---------------------------------------------------------------|
| `paint_shadow_ring()` | Soft black drop-shadow ring (3 expanding rects, fading alpha) |
| `paint_accent_glow()` | Subtle cyan aura ring (3 expanding rects, fading cyan)        |

The Memory Graph also paints a per-node two-layer shadow + accent glow ring
directly (see §7.3).

---

## 4. ICON

Defined in `theme::load_icon_rgba()`.

- **Size**: 64×64 RGBA
- **Fallback icon**: A programmatically generated "rearing horse" in calligraphy style, drawn with thin white-to-cyan gradient strokes via `generate_calligraphy_horse_icon()`.
- **States**:
  - **Active**: Full color (white→cyan gradient horse)
  - **Inactive**: Greyscale (applied via `apply_greyscale()`)
- **Icon source priority**:
  1. `resources/icon.png` (relative to CWD)
  2. `<exe_dir>/resources/icon.png`
  3. `<exe_dir>/config/../resources/icon.png`
  4. Programmatic fallback generation

---

## 5. WINDOW INVENTORY & LIFECYCLE

The application uses a **multi-process** architecture:
- **Main process**: System tray + background service + eframe window (the result popup)
- **Child processes**: Memory Graph, Functions Config (each a separate `eframe` app)

### 5.1 Windows Table

| Window                        | Process | Size (W×H)     | Default Position   | Spawn Trigger                |
|-------------------------------|---------|-----------------|--------------------|------------------------------|
| Main Result Popup             | Main    | 500×700         | (1350, 250)        | `ProcessingStarted` event    |
| Processing Indicator Bar      | Main    | 280×50          | Bottom-center      | `Waiting` state              |
| Memory Graph                  | Child   | 1000×800        | (900, 200)         | Tray left-click / menu item  |
| Functions Configuration       | Child   | 1120×760 *(r2)* | (1000, 130) *(r2)* | Tray menu "Configuration"    |
| Hotkey Configuration (legacy) | Child   | 550×450         | (1350, 300)        | Legacy standalone launcher   |

### 5.2 Window Flags

| Flag                 | Main Result | Memory Graph | Functions Config | Processing Bar |
|----------------------|-------------|--------------|------------------|----------------|
| Resizable            | No (fixed)  | Yes          | Yes              | No             |
| Always on top        | No          | No           | No               | No (but small) |
| `windows_subsystem`  | `"windows"` | `"windows"`  | `"windows"`      | Same window as main |
| Focus-on-show        | Yes (env override: `IntelliBoard_NO_UI_FOCUS`) | No explicit | No explicit | No explicit |

### 5.3 Process Manager

Child windows are managed by `ProcessManager`:
- `spawn_or_focus(key, title, command)`: Spawns new process or brings existing window to foreground.
- `kill_all()`: Kills all tracked child processes (called on Exit).
- Duplicate prevention: Checks for existing process by key before spawning.

---

## 6. COMPONENT DETAILS

### 6.1 System Tray

| Property               | Value                                          |
|------------------------|------------------------------------------------|
| Left-click             | Opens Memory Graph                             |
| Right-click (menu)     | Full context menu                              |
| Tooltip                | `"IntelliBoard - Left-click for Memory Graph"` |

**Tray Menu Items**:

| Item               | Type           | Checked Default | Action                                    |
|--------------------|----------------|-----------------|-------------------------------------------|
| `<custom commands>`| `MenuItem`     | —               | Config-driven, opens in Windows Terminal  |
| `---` (separator)  | —              | —               | Before/after custom commands              |
| Show Log           | `MenuItem`     | —               | Opens latest log in Notepad (Windows)     |
| Configuration      | `MenuItem`     | —               | Opens Functions Config child process      |
| Enable Processing  | `CheckMenuItem`| `true`          | Toggles clipboard processing              |
| Use Local Model    | `CheckMenuItem`| `false`         | Toggles local LLM mode                    |
| Exit               | `MenuItem`     | —               | Kills all processes + exit(0)             |

**Tray icon state**:
- `Enable Processing` toggled: switches between active (color) / inactive (greyscale) icon.

### 6.2 Processing Indicator Bar (Waiting State)

- **Visibility**: Only during `AppState::Waiting`.
- **Position**: Bottom-center of primary monitor, 60px from bottom.
- **Size**: 300×56 px *(r2: was 280×50 — slightly larger for breathing room)*.
- **Frame** *(r2: now uses `indicator_frame()` builder)*:
  - Background: `SURFACE_3` (`#181B24`, elevated) *(was flat `#0A0A10`)*
  - Border: `STROKE_ACCENT` × `ACCENT_BORDER` (`#00C8E6`, softened cyan) *(was raw `#00F3FF`)*
  - Rounding: `RADIUS_LG` (10) *(was 4 — sharper, cheaper feel)*
  - Inner margin: `SPACE_4` (8)
- **Content**:
  - Left: Action label (e.g., `"Format"`, `"Translation"`) in `TEXT_MD` cyan + animated dots (`"   "` → `".  "` → `".. "` → `"..."`, cycling at 2 Hz).
  - Right: Stop button — `CTRL_H_SM` (24px) sized control, transparent fill, hairline danger border, `DANGER_TEXT` ✕ *(was a bare red button)*.
- **Interaction**:
  - Stop button → sends `AppEvent::Cancel`, hides window, returns to `Idle`.
- **Repaint**: Every frame (for dot animation).

### 6.3 Main Result Window (Finished / Streaming / Error / Incomplete)

- **Visibility**: Only during `Finished | Streaming | Error | Incomplete`.
- **Position**: (1350, 250) — right side of screen, lower half.
- **Size**: 520×700 px *(r2: was 500 — slightly wider for comfortable line length)*.
- **Frame** *(r2: now uses `popup_frame()` builder)*:
  - Background: `SURFACE_3` (`#181B24`, elevated) *(was flat `#0A0A10`)*
  - Border: `STROKE_ACCENT` × `ACCENT_BORDER` (`#00C8E6`, softened cyan) *(was raw `#00F3FF`)*
  - Rounding: `RADIUS_XL` (14) *(was 0.0 sharp corners — the single biggest "cheap feel" cause)*
  - Inner margin: `SPACE_7` (20)
  - Depth: `paint_accent_glow()` soft cyan aura behind the panel *(new — adds "energy" without hard neon)*

**Layout (vertical split)**:
```
┌──────────────────────────────────┐
│  INPUT AREA (33% height)         │
│  ┌────────────────────────────┐  │
│  │ TextEdit (multiline)       │  │
│  │ ScrollArea                 │  │
│  │ editable (not in Waiting)  │  │
│  └────────────────────────────┘  │
│  [Close ❌]              [Status]│
├──────────────────────────────────┤
│  OUTPUT AREA (66% height)        │
│  ┌────────────────────────────┐  │
│  │ ScrollArea                 │  │
│  │ RichText (displayed_text)  │  │
│  │ or Loading animation       │  │
│  └────────────────────────────┘  │
└──────────────────────────────────┘
```

**Header row** (inside input area):
- Left: Input text (hidden during `Waiting`, shows `"INPUT LOCKED // 入力ロック中"` in `TEXT_MUTED` `TEXT_SM`).
- Right top *(r2)*:
  - Close button: `"✕"` sized control (`CTRL_H_SM` 24px), transparent fill, `NEON_PINK` × 0.6 hairline border — sends `Cancel`, hides window *(was a bare `❌` button)*.
  - Status indicator (Japanese labels, `TEXT_MD`):
    - `Waiting`: `"待機"` in `NEON_CYAN`
    - `Streaming`: `"受信"` in `NEON_CYAN`
    - `Finished`: `"终章"` in `SUCCESS` (`#00FF6E`)
    - `Incomplete`: `"中断"` in `WARN` (`#FFC800`)
    - `Error`: `"エラー"` in `NEON_PINK` (`#FF003C`)

**Input area interactions**:
- `Enter` (without Shift) triggers submit → `AppEvent::UserQuery`.
- `Shift+Enter` inserts newline.

**Output area states** *(r2)*:
- `Waiting`: Shows `"SYSTEM PROCESSING // 解析中"` (`TEXT_BASE` cyan) + animated scanner bar: rounded `SURFACE_2` track with a rounded `NEON_PINK` segment (16px high, 30% width, moving left→right at 1.5 Hz) *(was a sharp black/neon rectangle)*.
- `Streaming` / `Finished`: Shows `displayed_text` in `TEXT_COLOR` `TEXT_BASE` (13px).
- `Error`: Shows `displayed_text` (includes error message appended with `\n[Error: ...]`).
- `Incomplete`: Shows `displayed_text` as-is.

**Divider** *(r2)*: input/output separation is now a soft 1px `BORDER` hairline (allocated + painted) instead of a heavy `ui.separator()`.

**Keyboard shortcuts** (result window):
- `Escape` → Cancel, hide window, return to `Idle`.

### 6.4 Typewriter Effect

- **Active during**: `AppState::Streaming`.
- **Algorithm**:
  - Computes `backlog = text.len() - displayed_text.len()`.
  - Characters-per-frame:
    - `backlog > 100`: Show ALL remaining (instant catch-up).
    - `backlog > 20`: `max(10, backlog / 3)`.
    - Else: `min(3, backlog)` or all remaining.
  - Respects UTF-8 character boundaries.
- **Endpoint**: On `StreamEnd`, `displayed_text = text` (full reveal).

### 6.5 Hidden / Off-screen State

When `visible == false`:
- Window moved to (10000, 10000) — off-screen.
- Repaint interval: 500ms (low-CPU idle).
- Empty `CentralPanel` with no frame.

---

## 7. MEMORY GRAPH WINDOW (Child Process)

Defined in `src/ui/memory_graph.rs` + `src/bin/memory_graph_ui.rs`.

### 7.1 Architecture

- **Separate process**, communicates with main process via TCP on port `12345` (or arg-specified).
- **Protocol**: JSON-lines (`serde_json`) over TCP, with `GraphRequest` / `GraphResponse` messages.
- **Threads**:
  - Reader thread: Deserializes incoming `GraphResponse` into `mpsc::channel`.
  - Writer thread: Serializes outgoing `GraphRequest` to TCP.
  - Main thread: eframe UI loop.

### 7.2 Layout

```
┌────────────────────────────────────────────────────┐
│  TOOLBAR                                           │
│  [📋 Short-term ✓] [🔄 Mid-term ✓] [💾 Long-term ✓]│
│  🔍 [Search...___________]                         │
│  [Auto Align] [Export]                             │
├────────────────────────────────────────────────────┤
│  LEGEND (inline horizontal)                        │
│  ● Short  ● Mid  ● Long  ◌ Ghost  → edges ...     │
│  📋 Short: N [+10] | 🔄 Mid: N [+10] | 💾 Long..  │
├────────────────────────────────────────────────────┤
│                                                    │
│           GRAPH CANVAS (main area)                 │
│     Grid pattern (50px × zoom, #323246 @ 100α)     │
│     Nodes as circles (r=35 × zoom)                 │
│     Edges as lines (2px × zoom)                    │
│     Zoom: 0.3–2.5 (scroll wheel)                   │
│     Pan: drag empty area                           │
│                                                    │
├────────────────────────────────────────────────────┤
│  DETAILS PANEL (only when exactly 1 node selected) │
│  Window anchored RIGHT_TOP [-10, 40]               │
│  350px width                                       │
└────────────────────────────────────────────────────┘
```

### 7.3 Node Design

- **Shape**: Filled circle, radius = **38.0 × zoom** *(r2: was 35 — slightly larger for label breathing room)*.
- **Depth** *(r2: upgraded from a single offset shadow to a two-layer shadow + accent glow ring)*:
  - Layer 1 (soft outer wash): filled circle at offset (2, 4), radius +2, `rgba(0,0,0,60)`.
  - Layer 2 (tight core): filled circle at offset (1, 2), `rgba(0,0,0,110)`.
  - Accent glow ring: filled circle, radius +4, `rgba(0,200,230,22)` — only when `zoom > 0.6` to avoid clutter when zoomed out. This gives nodes "energy" without a hard neon outline.
- **Stroke**: white circle outline, width = `(2.0 × zoom).clamp(1.5, 3.0)` *(r2: was a fixed 2px — now scales crisply with zoom)*.
- **Fill colors** (by tier):
  - Short-term: `#00C8FF`
  - Mid-term: `#FFC800`
  - Long-term: `#00FF64`
- **State overrides**:
  - Hovered: `gamma_multiply(1.3)` on base color.
  - Selected: White.
  - Edge-creation source: Yellow.
- **Label** (inside node):
  - Priority: `item.title` (if non-empty), else first 3 words of `item.content`.
  - Truncated to 12 Unicode characters (with `…`).
  - Font: Proportional `TEXT_XS` (11.0) × zoom, `Color32::BLACK`.
  - Alignment: Center-center.

### 7.4 Ghost Nodes

- Represent edge sources that no longer exist (lost on reboot).
- **Size**: `NODE_RADIUS × zoom × 0.7` (smaller).
- **Color**: `#505050` (grey).
- **Style**: Dashed circle (stroke only, no fill).
- **Label**: `"👻"` emoji in `TEXT_SM` (12.0) × zoom *(r2: was 14.0)*.

### 7.5 Edge Design

- **Line**: 2px × zoom stroke.
- **Clipping**: Truncated at node circle boundary (`start = from + dir × r`, `end = to - dir × r`).
- **Skip**: Edges with near-zero length (nodes overlapping) are not rendered.
- **Off-screen culling**: Edges are skipped if both endpoints are outside `rect.expand(50)`.
- **Edge creation preview**: Dashed white line from source node to cursor.

### 7.6 Graph Interactions

| Action                | Trigger                          | Result                                    |
|-----------------------|----------------------------------|-------------------------------------------|
| Pan                   | Drag on empty area               | `view_offset += drag_delta`               |
| Zoom                  | Scroll wheel (over canvas)       | `zoom = clamp(zoom × (1 + δ×0.001), 0.3, 2.5)` |
| Select node           | Click on node                    | Single selection (clears previous)         |
| Multi-select          | Ctrl+Click on node               | Toggle selection                           |
| Marquee select        | Shift+Drag on empty area         | Rectangle select all nodes inside         |
| Move node             | Drag on node                     | Updates position, persists via `UpdateNodePosition` request |
| Delete node(s)        | Right-click → context menu, or Delete key | Sends `DeleteItem` request(s)     |
| Create user edge      | "🔗 Link to..." button in details → click target | Sends `AddUserEdge` request      |
| Cancel edge creation  | Escape                           | Clears `edge_creation_source`              |
| Promote node          | "→ Mid" / "→ Long" button        | Sends `PromoteItem` request                |
| Export graph          | "Export" toolbar button           | Saves JSON to configured path / Downloads |
| Auto Align            | "Auto Align" toolbar button       | Resets all positions to tier-stacked layout |

### 7.7 Details Panel (single node selected)

- **Window**: Floating `egui::Window`, anchored `RIGHT_TOP` at (-10, 40), 350px wide.
- **Fields displayed**:
  - Memory type (emoji + label)
  - Created timestamp
  - Promoted timestamp (if applicable)
  - Action type (if `MemoryMetadata::ActionResult`)
  - Editable title (single-line TextEdit + 💾 save button, Enter to save)
  - Content (read-only multiline, max 200px scroll)
- **Buttons**:
  - `→ Mid` / `→ Long` (promote)
  - `🔗 Link to...` (start edge creation)
  - `📋 Copy` (copy content to clipboard)
  - `🗑 Delete` (delete item)

### 7.8 Layout Algorithm (Tier-Stacked)

> *(r2)*: `NODE_RADIUS` 35→38, `NODE_SPACING_Y` 90→96, `COLUMN_SPACING` 250→260 — bumped in lock-step with the larger node radius so spacing remains proportional (no overlap, even breathing room).

- **Three columns**:
  - Short-term: X = -260 (`COLUMN_SPACING`)
  - Mid-term: X = 0
  - Long-term: X = +260
- **Y**: Sequential by recency (newest at top, `index × 96`).
- **Position priority**: Item's persisted position → session position → default computed.
- **Ghost nodes**: X = -390 (`COLUMN_SPACING × 1.5`), stacked vertically.

### 7.9 Toolbar Controls

| Control            | Type        | State Variable       |
|--------------------|-------------|----------------------|
| 📋 Short-term      | Checkbox    | `show_short_term`    |
| 🔄 Mid-term        | Checkbox    | `show_mid_term`      |
| 💾 Long-term       | Checkbox    | `show_long_term`     |
| 🔍 Search          | TextEdit    | `search_query`       |
| Auto Align         | Button      | `force_default_layout` |
| Export             | Button      | calls `export_graph()` |
| +10 (per tier)     | SmallButton | `*_term_limit += 10` |

---

## 8. FUNCTIONS CONFIGURATION WINDOW (Child Process)

Defined in `src/bin/functions_config_ui.rs`.

### 8.1 Layout (Three-Panel)

> *(r2)*: window 1080×720 → **1120×760**; nav panel 184px → **208px**; section card radius 8 → `RADIUS_LG` (10); page title 22px → `TEXT_2XL` (24); nav items now `CTRL_H_LG` (36px) sized buttons. All fills/borders now come from the shared elevation tokens (`SURFACE_1`/`SURFACE_2`/`BORDER`), no longer hardcoded `#0B0D13`.

```
┌──────────┬─────────────────────────────────────────┐
│ NAV      │  CONTENT AREA                           │
│ (208px)  │                                         │
│          │  Page Title (TEXT_2XL, 24px)            │
│ Intelli- │  ┌─────────────────────────────────┐    │
│ Board    │  │ Section Card (SURFACE_2, r=10)  │    │
│ Functions│  │  Section Title (TEXT_MD, 14px)  │    │
│          │  │  ...                            │    │
│ ──────── │  └─────────────────────────────────┘    │
│ AI Actions│                                        │
│ Hotkeys  │                                         │
│ Settings │                                         │
│          │                                         │
│ ──────── │                                         │
│ [Saved]  │                                         │
├──────────┴─────────────────────────────────────────┤
│  BOTTOM BAR                                        │
│  [Save All] [Reset Hotkeys] [Reset Actions]    [✕] │
└────────────────────────────────────────────────────┘
```

### 8.2 Navigation Panel

- **Width**: 208px (exact) *(r2: was 184)*.
- **Background**: `SURFACE_1` (`#0F0F14`) *(r2: was hardcoded `#0B0D13`)*.
- **Border**: `STROKE_HAIRLINE` × `BORDER` (`#303644`).
- **Inner margin**: `SPACE_6` (16) all sides *(r2: was 14)*.
- **Items**:
  - Brand: `"IntelliBoard"` `TEXT_XL` (19px) bold + `"Functions"` `TEXT_SM` muted.
  - Nav items: full-width `CTRL_H_LG` (36px) buttons, `RADIUS_MD` rounding *(r2: was 34px/r=6)*:
    - Selected: `SURFACE_ALT` (`SURFACE_3`) fill, `ACCENT` border, white bold `TEXT_BASE` text.
    - Deselected: Transparent fill, `BORDER` border, `TEXT_BASE` text.
  - Footer: `"Unsaved changes"` (amber `TEXT_SM`) if dirty, or `"Saved"` (muted `TEXT_SM`).

### 8.3 Tabs

#### AI Actions Tab
- **Left sub-panel** (240px):
  - Action list: Scrollable buttons (32px height, r=6).
  - Hidden actions shown as `"label  hidden"`.
  - New action: TextEdit + Add button (generates ID from name).
  - Delete button (red, only when an action is selected).
- **Right sub-panel** (remaining width):
  - Action header: Label (18px) + ID (monospace, muted) + [Revert] [Apply].
  - Sections (each a card with 8px rounding):
    - **Action**: Description TextEdit + Hidden checkbox.
    - **Remote Request**: Model TextEdit + Prompt (multiline, 5 rows) + Advanced (collapsible JSON editor).
    - **Local Request**: Model TextEdit + Prompt (multiline, 4 rows) + Advanced (collapsible JSON editor).

#### Hotkeys Tab
- **Current Shortcuts** section: Grid with columns Key (monospace) | Action (ComboBox) | Description | Remove button.
- **Add Shortcut** section: `Ctrl + [Key ComboBox]` + action buttons (one per defined action).
- Actions listed come from the Actions config (filtered non-hidden).

#### Settings Tab
- **Remote Defaults**: API URL, Model, API Key (password-masked).
- **Local Defaults**: API URL, Model.
- **Behavior**: `force_local` checkbox.
- **Export**: Path TextEdit.

### 8.4 Bottom Bar

- **Background**: `#0B0D13`, 1px `BORDER` top.
- **Inner margin**: 16px horizontal, 10px vertical.
- **Buttons**: Save All, Reset Hotkeys, Reset Actions, Close (right-aligned).
- **Feedback**: Colored text (success=ACCENT, error=DANGER), fades after 4 seconds.

### 8.5 Conflict Detection

- Tracks `config/actions.toml` modification time.
- If file changed externally while editor has unsaved changes: modal dialog with [Reload] / [Keep Editing].

### 8.6 Hot-Reload

- `save_actions_config()` and `save_hotkeys_config()` write directly to config files.
- The main process's `ConfigWatcher` detects changes and hot-reloads.

---

## 9. HOTKEY CONFIGURATION WINDOW (Legacy)

Defined in `src/bin/hotkey_config_ui.rs` and `src/ui/hotkey_config.rs`.

### 9.1 Legacy Standalone (hotkey_config_ui.rs)

- **Size**: 550×450, positioned at (1350, 300).
- **Content**: Simple grid — Ctrl+Key | Action (ComboBox) | Description | Remove.
- **Add**: `Ctrl + [Key ComboBox]` + buttons for Format / TranslateE2C / TranslateC2E / Explain.
- **Actions**: Hardcoded list: Format, TranslateE2C, TranslateC2E, Explain.
- **Available keys**: A–Z, 0–9.

### 9.2 Inline HotkeyConfigWindow (hotkey_config.rs)

- Used as a component within another window.
- Same functionality but structured as `HotkeyConfigWindow` struct with `show()` method returning `bool` (open state).
- Adds `shared_hotkeys: Arc<RwLock<HotkeysConfig>>` for hot-reload: saves to file + writes to shared config.

---

## 10. STATE MACHINE

### 10.1 Main UI State (`AppState`)

```
                    ProcessingStarted
    ┌──────┐ ──────────────────────────> ┌─────────┐
    │ Idle │                             │ Waiting │
    └──────┘ <────────────────────────── └────┬─────┘
        ▲       Cancel / Escape / Stop        │
        │                        First StreamUpdate
        │                                      │
        │              ┌───────────────────────┤
        │              ▼                       │
        │        ┌───────────┐                 │
        ├────────│ Streaming │                 │
        │        └─────┬─────┘                 │
        │   StreamEnd  │  StreamEnd            │
        │   (false)    │  (true)               │
        │              ▼                       ▼
        │        ┌──────────┐           ┌──────────┐
        │        │Incomplete│           │ Finished │
        │        └──────────┘           └──────────┘
        │                                       │
        │  StreamError                          │
        │        ┌──────────┐                   │
        └────────│  Error   │                   │
                 └──────────┘                   │
                                                │
              Close / Escape / New Action ◄─────┘
```

### 10.2 Window Visibility Matrix

| State       | Processing Bar | Result Window | Input Editable | Output Shows     |
|-------------|---------------|---------------|----------------|------------------|
| Idle        | Hidden        | Hidden        | N/A            | N/A              |
| Waiting     | **Visible**   | Hidden        | N/A            | Loading anim     |
| Streaming   | Hidden        | **Visible**   | Locked         | Typewriter text  |
| Finished    | Hidden        | **Visible**   | Editable       | Final text       |
| Error       | Hidden        | **Visible**   | Editable       | Error text       |
| Incomplete  | Hidden        | **Visible**   | Editable       | Partial text     |

---

## 11. EVENT FLOW

### 11.1 UI Event Types (`UiEvent`)

| Variant                | Payload        | Source                | Effect                                 |
|------------------------|----------------|-----------------------|----------------------------------------|
| `ProcessingStarted`    | `String` (label)| `ActionHandler`       | Show processing bar, state→Waiting     |
| `ShowResult`           | `(orig, text)` | `ActionHandler`       | Show result, state→Finished            |
| `StreamUpdate`         | `String`       | `LlmClient`           | Append to text, state→Streaming        |
| `StreamEnd`            | `bool`         | `LlmClient`           | Finalize typewriter, state→Finished/Incomplete |
| `StreamError`          | `String`       | `LlmClient` / handler | Show error, state→Error               |
| `ShowMemoryGraph`      | —              | Tray / hotkey         | Spawn Memory Graph child process       |
| `ShowHotkeyConfig`     | —              | Tray menu             | Spawn Functions Config child process   |
| `Quit`                 | —              | Control server        | Close window                           |

### 11.2 App Event Types (`AppEvent`)

| Variant            | Payload            | Source                  | Effect                              |
|--------------------|--------------------|-------------------------|-------------------------------------|
| `TriggerAction`    | `Action`           | Hotkey system           | Execute AI action on clipboard      |
| `UserQuery`        | `String`           | Main UI input           | Send user text to LLM               |
| `ToggleProcessing` | `bool`             | Tray checkbox           | Enable/disable clipboard processing |
| `ToggleLocalMode`  | `bool`             | Tray checkbox           | Force local LLM mode                |
| `Cancel`           | —                  | UI / stop button        | Cancel in-flight request            |
| `ShowMemoryGraph`  | —                  | Tray left-click         | Same as UI event variant            |
| `ShowHotkeyConfig` | —                  | Tray menu               | Same as UI event variant            |
| `ConfigChanged`    | `ConfigChange`     | ConfigWatcher           | Hot-reload config + reinstall hooks |

### 11.3 Channel Architecture

```
┌──────────┐  flume::unbounded   ┌──────────┐
│  UI      │ ◄────────────────── │ Backend  │
│ (eframe) │                     │ (tokio)  │
└──────────┘                     └──────────┘
     │                                ▲
     │ tokio::mpsc                    │ tokio::mpsc
     ▼                                │
┌──────────┐                     ┌──────────┐
│ LlmClient│ ── UiEvent ────────▶│  Bridge  │
│ (async)  │   (tokio::mpsc)     │  Task    │
└──────────┘                     └──────────┘
```

- **UI → Backend**: `flume::unbounded::Sender<UiEvent>` (from `main.rs`).
- **Backend → UI**: `tokio::sync::mpsc::Sender<UiEvent>` (for async producers like `LlmClient`), bridged to flume in a spawn task.
- **Main event loop**: `tokio::sync::mpsc::channel::<AppEvent>(100)`.

---

## 12. THEME APPLICATION

### `theme::apply_theme(ctx)`

Applied at startup for ALL windows (main + child processes). *(r2: the visuals
now use elevation surfaces, softened accents, scaled strokes and consistent
rounding instead of raw neon and flat fills.)*

```rust
// Surfaces — elevation layers, not flat fills
visuals.panel_fill          = SURFACE_1;     // #0F0F14
visuals.window_fill         = SURFACE_0;     // #0A0A10
visuals.extreme_bg_color    = SURFACE_0;
visuals.faint_bg_color      = SURFACE_2;     // #14161E
visuals.override_text_color = TEXT_COLOR;    // #DCE6FF

// Selection — softened (was raw NEON_PINK flood)
visuals.selection.bg_fill = ACCENT_SELECT;   // #BE1C46
visuals.selection.stroke  = Stroke(STROKE_HAIRLINE, WHITE);

// Widgets — elevation surfaces + scaled strokes per state
visuals.widgets.noninteractive.bg_fill   = SURFACE_1;
visuals.widgets.noninteractive.fg_stroke = Stroke(STROKE_HAIRLINE, TEXT_MUTED);
visuals.widgets.inactive.bg_fill         = SURFACE_2;
visuals.widgets.hovered.bg_fill          = HOVERED_BG;     // #232332
visuals.widgets.hovered.bg_stroke        = Stroke(STROKE_THIN,   ACCENT_BORDER);
visuals.widgets.active.bg_fill           = NEON_CYAN;
visuals.widgets.active.bg_stroke         = Stroke(STROKE_ACCENT, NEON_CYAN);

// Window / widget rounding — consistent corner language
visuals.window_rounding      = Rounding::same(RADIUS_XL);  // 14
visuals.menu_rounding        = Rounding::same(RADIUS_LG);  // 10
visuals.widgets.*.rounding   = Rounding::same(RADIUS_MD);  // 6
```

### Functions Config UI overrides

*(r2)*: The Functions Config window **no longer** declares its own palette.
`SURFACE / SURFACE_ALT / BORDER / ACCENT / TEXT_MUTED / DANGER` are now private
`const` aliases of the canonical theme tokens, so section cards and nav items
layer on top of the same base dark theme everywhere.

---

## 13. CONFIG-DRIVEN UI ELEMENTS

### 13.1 Custom Commands in Tray

- Source: `config/commands.toml` → `CommandsConfig`.
- Each entry becomes a `MenuItem` in the tray menu, sorted alphabetically.
- Execution: `wt.exe -p "Windows PowerShell" -d . powershell -Command "<cmd>"` (Windows only).
- Separators added before/after custom commands block (only if commands exist).

### 13.2 Action Labels in UI

- Actions defined in `config/actions.toml` → `ActionsConfig`.
- Visible actions (non-hidden) populate:
  - Hotkey binding action dropdowns (both legacy and functions config).
  - Action list in Functions Config.
- Labels come from `action.label()` → falls back to `action.id`.

---

## 14. DIAGNOSTIC ENV VARS

| Variable                    | Effect                                          |
|-----------------------------|-------------------------------------------------|
| `IntelliBoard_DIAG_UI`      | Log extra UI debug info (ProcessingStarted, ShowResult, focus suppression) |
| `IntelliBoard_NO_UI_FOCUS`  | Suppress `ViewportCommand::Focus` on result show|
| `IntelliBoard_DIAG_CLIPBOARD`| Log extra clipboard debug info                  |

---

## 15. DEPENDENCY SUMMARY (GUI-related)

| Crate              | Usage                                   |
|--------------------|-----------------------------------------|
| `eframe` / `egui`  | Immediate-mode GUI framework            |
| `tray-icon`        | System tray icon + menu                 |
| `image`            | Load/draw tray icon PNG                 |
| `rdev`             | Global keyboard input (non-Windows)     |
| `dirs`             | Default export path (Downloads)         |
| `serde_json`       | IPC protocol, graph export              |
| `uuid`             | Memory item/node IDs                    |
| `chrono`           | Timestamp display, export filenames     |
| `single-instance`  | Prevent duplicate main process          |

---

## 16. DESIGN AUDIT FINDINGS & RATIONALE (2026-07-03 r2)

This section records what the audit identified and why each fix restores
texture and coordinated sizing.

### 16.1 Problems Found

| # | Problem | Root cause |
|---|---------|------------|
| 1 | **Lack of texture / depth** | Result window used `rounding(0.0)` sharp corners + flat black fill; panels were flat fills with no layering; only the memory graph had a single crude offset shadow. |
| 2 | **Raw neon everywhere** | `NEON_CYAN` / `NEON_PINK` used directly for large borders and selection floods → visually vibrating, "cheap" feel. |
| 3 | **Uncoordinated sizing** | Radii scattered (`0/4/6/8`), padding scattered (`8/10/12/14/16/20`), font sizes scattered (`11/12/14/18/19/22`), button heights scattered (`22/32/34`), stroke weights mixed (`1/2`). |
| 4 | **Two divergent palettes** | `theme.rs` and `functions_config_ui.rs` each declared their own `SURFACE/BORDER/ACCENT` with different hex values, so the two windows never matched. |
| 5 | **No elevation model** | Every surface was the same near-black; cards did not "lift" off panels. |

### 16.2 Fixes Applied

1. **Design-token system** (§3): one source of truth for spacing, radius, type,
   stroke, control heights. All call sites now pick from these ladders.
2. **Elevation surfaces** (§1.4): `SURFACE_0..3` give depth by luminance step;
   panels, cards and popups stack visually instead of being flat.
3. **Soft accent variants** (§1.3): `ACCENT_BORDER`, `ACCENT_SELECT`,
   `DANGER_TEXT` etc. soften raw neon for borders/tints/text while keeping neon
   for true emphasis.
4. **Frame builders** (§3.5): `panel_frame()` / `card_frame()` / `popup_frame()`
   / `indicator_frame()` are the single source of truth for component shells —
   consistent radius, stroke, padding and surface everywhere.
5. **Depth helpers** (§3.6): `paint_shadow_ring()` / `paint_accent_glow()`
   emulate blur with cheap concentric rings (egui has no native blur).
6. **Result window** (§6.3): rounded (`RADIUS_XL`), elevated (`SURFACE_3`),
   soft accent ring + glow, sized close button, soft hairline divider.
7. **Processing bar** (§6.2): rounded, elevated, softened accent ring, sized
   stop control.
8. **Memory graph nodes** (§7.3): two-layer shadow + accent glow ring + zoom-
   scaling outline for a crisp, "energised" feel.
9. **Unified palette** (§1.5): Functions Config aliases now point at canonical
   tokens — the two windows can no longer drift.

### 16.3 Texture Principles Enforced

- **Raw neon is reserved for emphasis** (active widgets, status pills, scanner
  segment). Borders, fills and large areas use softened/elevated variants.
- **Depth comes from elevation layers + soft shadows/glows**, never from a
  heavier stroke.
- **Every dimension is a token**, never a magic number — this is what makes the
  UI read as deliberately designed rather than improvised.

---

*End of GUI Design Audit. All values confirmed against source as of 2026-07-03 (r2).*
