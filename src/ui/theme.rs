//! Shared theme and font configuration for IntelliBoard UI
//!
//! Provides the "Japan 2046" cyberpunk aesthetic used across all UI components.
//!
//! Design system (see `docs/GUI_DESIGN_AUDIT.md` §1.6–§1.10):
//! - **Spacing scale** (`SPACE_*`, base 4px) drives all margins/gaps.
//! - **Radius scale** (`RADIUS_*`) gives consistent corner language.
//! - **Type scale** (`TEXT_*`) is an 8:8 ratio ladder.
//! - **Stroke scale** (`STROKE_*`) harmonises hairlines vs accents.
//! - **Elevation surfaces** (`SURFACE_*`) layer depth instead of flat fills.
//! - **Soft accents** (`*_SOFT`, `*_DIM`) add glow/atmosphere without raw neon.
//! - **Frame builders** (`panel_frame`, `card_frame`, `popup_frame`, `shadow_*`)
//!   are the single source of truth for component shells.

use eframe::egui;
use log::info;

// =============================================================================
// 1. CORE PALETTE  (Japan 2046 — Cyber-Japonism)
// =============================================================================
//
// Naming convention:
//   <ROLE>[_VARIANT]
//     ROLE    = semantic (BG / PANEL / TEXT / ACCENT / DANGER / ...)
//     VARIANT = raw | SOFT (low-saturation glow) | DIM (faint wash)

pub const BG_COLOR: egui::Color32 = egui::Color32::from_rgb(10, 10, 16);       // Deep Cyber Night
pub const PANEL_COLOR: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);    // Slightly lighter

pub const NEON_CYAN: egui::Color32 = egui::Color32::from_rgb(0, 243, 255);     // Cyber Cyan
pub const NEON_PINK: egui::Color32 = egui::Color32::from_rgb(255, 0, 60);      // Cyber Pink

pub const TEXT_COLOR: egui::Color32 = egui::Color32::from_rgb(220, 230, 255);  // Cool White
pub const INACTIVE_BG: egui::Color32 = egui::Color32::from_rgb(25, 25, 35);
pub const HOVERED_BG: egui::Color32 = egui::Color32::from_rgb(35, 35, 50);

// Edge colors for memory graph
pub const EDGE_ACTION: egui::Color32 = egui::Color32::from_rgb(0, 200, 255);       // Blue - action result
pub const EDGE_PROMOTION: egui::Color32 = egui::Color32::from_rgb(0, 255, 100);    // Green - promotion
pub const EDGE_USER: egui::Color32 = egui::Color32::from_rgb(255, 200, 0);         // Yellow - user-created
pub const EDGE_FORMAT: egui::Color32 = egui::Color32::from_rgb(255, 128, 0);       // Orange - format

// -----------------------------------------------------------------------------
// 1.7  Soft / atmospheric accent variants
//      Raw neon at full saturation is harsh on large areas; the *_SOFT tokens
//      are used for borders, glows and tints to give the UI "atmosphere".
// -----------------------------------------------------------------------------

/// Cyan border for cards / popups — saturated enough to read as accent,
/// softened vs. raw `NEON_CYAN` so it does not vibrate against the dark fill.
pub const ACCENT_BORDER: egui::Color32 = egui::Color32::from_rgb(0, 200, 230);
/// Faint cyan wash for backgrounds (selected items, hover tints).
pub const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(12, 40, 52);
/// Softened selection background (replaces raw `NEON_PINK` flood).
pub const ACCENT_SELECT: egui::Color32 = egui::Color32::from_rgb(190, 28, 70);
/// Subtle red used for "danger" text — readable on dark surfaces.
pub const DANGER_TEXT: egui::Color32 = egui::Color32::from_rgb(255, 96, 110);
/// Secondary / muted text color.
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(145, 154, 174);
/// Matrix-style "success" green.
pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(0, 255, 110);
/// Warning / incomplete amber.
pub const WARN: egui::Color32 = egui::Color32::from_rgb(255, 200, 0);

// =============================================================================
// 2. ELEVATION SURFACES  (depth, not flat fills)
// =============================================================================
//
// Each successive level lifts one step in luminance so panels, cards and
// popups read as stacked layers. Layering replaces flat neon for "texture".
//
//   SURFACE_0  — root background (deepest)   ~ BG_COLOR
//   SURFACE_1  — top-level panels            ~ PANEL_COLOR
//   SURFACE_2  — in-panel cards              (lifts off the panel)
//   SURFACE_3  — floating windows / popups   (highest)
//
// `functions_config_ui.rs` previously declared its own SURFACE/SURFACE_ALT
// constants; those are now aliased to these canonical tokens below.

pub const SURFACE_0: egui::Color32 = egui::Color32::from_rgb(10, 10, 16);    // root bg
pub const SURFACE_1: egui::Color32 = egui::Color32::from_rgb(15, 15, 20);    // panel
pub const SURFACE_2: egui::Color32 = egui::Color32::from_rgb(20, 22, 30);    // card
pub const SURFACE_3: egui::Color32 = egui::Color32::from_rgb(24, 27, 36);    // floating

/// Subtle hairline that separates elevation layers. Used as the canonical
/// border color across the whole UI (replaces the scattered `BORDER` token in
/// `functions_config_ui.rs`).
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(48, 54, 68);
/// Slightly stronger border used when a card is focused / hovered.
pub const BORDER_STRONG: egui::Color32 = egui::Color32::from_rgb(0, 200, 230);

// =============================================================================
// 3. SPACING SCALE  (base unit = 4px, 4:8 rhythm)
// =============================================================================
//
// All paddings, margins and component gaps should pick from this ladder.
// Half-steps (2, 6, 10) are allowed for tight inline alignment.

pub const SPACE_0: f32 = 0.0;
pub const SPACE_1: f32 = 2.0;   // hairline gap
pub const SPACE_2: f32 = 4.0;   // base unit
pub const SPACE_3: f32 = 6.0;
pub const SPACE_4: f32 = 8.0;   // default inner control gap
pub const SPACE_5: f32 = 12.0;  // section spacing
pub const SPACE_6: f32 = 16.0;  // panel padding
pub const SPACE_7: f32 = 20.0;  // popup padding
pub const SPACE_8: f32 = 24.0;  // page gutter
pub const SPACE_9: f32 = 32.0;  // large block separation

// =============================================================================
// 4. RADIUS SCALE  (consistent corner language)
// =============================================================================
//
//   RADIUS_SM  — small controls, chips, badges          4
//   RADIUS_MD  — buttons, list rows                      6
//   RADIUS_LG  — cards, section containers               10
//   RADIUS_XL  — popups, modals                          14
//   RADIUS_PILL — fully rounded (tags, status pills)     999

pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 10.0;
pub const RADIUS_XL: f32 = 14.0;
pub const RADIUS_PILL: f32 = 999.0;

// =============================================================================
// 5. TYPOGRAPHY SCALE  (1.2 ratio ladder, base 13)
// =============================================================================
//
// Every visible label should pick from this ladder so font sizes are
// harmonious across windows. Sizes are in logical px (before DPI scaling).

pub const TEXT_XS: f32 = 11.0;   // legend chips, captions, micro-labels
pub const TEXT_SM: f32 = 12.0;   // JSON / monospace editor, table secondary
pub const TEXT_BASE: f32 = 13.0; // default body, input/output text
pub const TEXT_MD: f32 = 14.0;   // emphasized body, processing bar label
pub const TEXT_LG: f32 = 16.0;   // section titles, action header
pub const TEXT_XL: f32 = 19.0;   // page title, nav brand
pub const TEXT_2XL: f32 = 24.0;  // large headings

// =============================================================================
// 6. STROKE SCALE  (hairline → accent → emphasis)
// =============================================================================

pub const STROKE_HAIRLINE: f32 = 1.0;   // dividers, card outlines
pub const STROKE_THIN: f32 = 1.5;       // default borders
pub const STROKE_ACCENT: f32 = 2.0;     // accent borders (cyan ring)
pub const STROKE_BOLD: f32 = 3.0;       // emphasized frames, popup edges

// Embedded icon data (generated by build.rs, fallback to programmatic generation)
const ICON_SIZE: u32 = 64;

/// Load the app icon as RGBA bytes (for tray and window icons)
/// Returns (rgba_bytes, width, height)
pub fn load_icon_rgba() -> (Vec<u8>, u32, u32) {
    // Try multiple locations for the icon
    let paths_to_try = [
        // Relative to current directory
        std::path::PathBuf::from("resources/icon.png"),
        // Relative to executable
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("resources/icon.png")))
            .unwrap_or_default(),
        // Config directory (same as config files)
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("config/../resources/icon.png")))
            .unwrap_or_default(),
    ];
    
    for path in &paths_to_try {
        if path.exists() {
            if let Ok(img) = image::open(path) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                return (rgba.into_raw(), w, h);
            }
        }
    }
    
    // Fallback: generate vibrant calligraphy horse programmatically
    generate_calligraphy_horse_icon()
}

/// Generate a HEROIC rearing horse icon programmatically (fallback)
/// Thin detailed strokes with white-to-cyan gradient
fn generate_calligraphy_horse_icon() -> (Vec<u8>, u32, u32) {
    let size = ICON_SIZE;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    
    // Helper to draw thin stroke with white-cyan gradient
    let draw_stroke = |rgba: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color_t: f32| {
        // Gradient: pure white (1.0) to electric cyan (0.0)
        let white = (255.0f32, 255.0f32, 255.0f32);
        let cyan = (0.0f32, 230.0f32, 255.0f32);
        let r = (cyan.0 + (white.0 - cyan.0) * color_t) as u8;
        let g = (cyan.1 + (white.1 - cyan.1) * color_t) as u8;
        let b = (cyan.2 + (white.2 - cyan.2) * color_t) as u8;
        
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()) * 4.0) as i32 + 1;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let cx = x0 + (x1 - x0) * t;
            let cy = y0 + (y1 - y0) * t;
            let w = width * (0.7 + 0.4 * (t * std::f32::consts::PI).sin());
            
            for dy in (-(w as i32) - 1)..=(w as i32 + 1) {
                for dx in (-(w as i32) - 1)..=(w as i32 + 1) {
                    let px = (cx + dx as f32) as i32;
                    let py = (cy + dy as f32) as i32;
                    if px >= 0 && px < size as i32 && py >= 0 && py < size as i32 {
                        let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                        if dist <= w {
                            let alpha = if dist > w - 0.8 { ((w - dist) / 0.8 * 255.0).min(255.0) as u8 } else { 255 };
                            let idx = ((py as u32 * size + px as u32) * 4) as usize;
                            let old_a = rgba[idx + 3] as u32;
                            let new_a = alpha as u32;
                            let out_a = old_a + new_a - (old_a * new_a / 255);
                            if out_a > 0 {
                                let blend = new_a as f32 / out_a as f32;
                                rgba[idx] = ((rgba[idx] as f32 * (1.0 - blend) + r as f32 * blend)) as u8;
                                rgba[idx + 1] = ((rgba[idx + 1] as f32 * (1.0 - blend) + g as f32 * blend)) as u8;
                                rgba[idx + 2] = ((rgba[idx + 2] as f32 * (1.0 - blend) + b as f32 * blend)) as u8;
                                rgba[idx + 3] = out_a.min(255) as u8;
                            }
                        }
                    }
                }
            }
        }
    };
    
    // HEROIC REARING HORSE - thin detailed strokes, white-cyan gradient
    
    // Back hooves (cyan)
    draw_stroke(&mut rgba, 42.0, 60.0, 44.0, 62.0, 1.0, 0.15);
    draw_stroke(&mut rgba, 54.0, 58.0, 56.0, 62.0, 1.0, 0.1);
    
    // Back legs
    draw_stroke(&mut rgba, 42.0, 48.0, 42.0, 60.0, 1.4, 0.2);
    draw_stroke(&mut rgba, 44.0, 40.0, 42.0, 48.0, 1.5, 0.25);
    draw_stroke(&mut rgba, 52.0, 46.0, 54.0, 58.0, 1.3, 0.15);
    draw_stroke(&mut rgba, 50.0, 38.0, 52.0, 46.0, 1.4, 0.2);
    
    // Hindquarters
    draw_stroke(&mut rgba, 48.0, 36.0, 54.0, 32.0, 1.8, 0.25);
    draw_stroke(&mut rgba, 46.0, 38.0, 50.0, 36.0, 1.6, 0.3);
    
    // Tail
    draw_stroke(&mut rgba, 54.0, 32.0, 58.0, 26.0, 1.2, 0.1);
    draw_stroke(&mut rgba, 58.0, 26.0, 60.0, 18.0, 1.0, 0.05);
    draw_stroke(&mut rgba, 60.0, 18.0, 58.0, 10.0, 0.8, 0.0);
    draw_stroke(&mut rgba, 58.0, 10.0, 54.0, 4.0, 0.7, 0.0);
    draw_stroke(&mut rgba, 56.0, 28.0, 60.0, 22.0, 0.9, 0.08);
    draw_stroke(&mut rgba, 60.0, 22.0, 62.0, 14.0, 0.8, 0.02);
    draw_stroke(&mut rgba, 62.0, 14.0, 60.0, 6.0, 0.7, 0.0);
    draw_stroke(&mut rgba, 58.0, 24.0, 62.0, 18.0, 0.8, 0.05);
    draw_stroke(&mut rgba, 62.0, 18.0, 64.0, 10.0, 0.6, 0.0);
    
    // Body
    draw_stroke(&mut rgba, 48.0, 36.0, 40.0, 30.0, 2.0, 0.35);
    draw_stroke(&mut rgba, 40.0, 30.0, 32.0, 24.0, 1.9, 0.45);
    draw_stroke(&mut rgba, 36.0, 32.0, 46.0, 38.0, 1.8, 0.35);
    draw_stroke(&mut rgba, 34.0, 28.0, 42.0, 34.0, 1.6, 0.4);
    
    // Chest
    draw_stroke(&mut rgba, 32.0, 24.0, 28.0, 28.0, 1.8, 0.55);
    draw_stroke(&mut rgba, 28.0, 28.0, 26.0, 34.0, 1.6, 0.6);
    
    // Front legs RAISED
    draw_stroke(&mut rgba, 26.0, 34.0, 20.0, 40.0, 1.4, 0.65);
    draw_stroke(&mut rgba, 20.0, 40.0, 12.0, 42.0, 1.2, 0.7);
    draw_stroke(&mut rgba, 12.0, 42.0, 6.0, 38.0, 1.0, 0.75);
    draw_stroke(&mut rgba, 6.0, 38.0, 4.0, 34.0, 0.8, 0.8);
    draw_stroke(&mut rgba, 28.0, 30.0, 32.0, 38.0, 1.3, 0.6);
    draw_stroke(&mut rgba, 32.0, 38.0, 28.0, 46.0, 1.1, 0.65);
    draw_stroke(&mut rgba, 28.0, 46.0, 22.0, 44.0, 0.9, 0.7);
    
    // Neck
    draw_stroke(&mut rgba, 32.0, 24.0, 28.0, 18.0, 1.7, 0.6);
    draw_stroke(&mut rgba, 28.0, 18.0, 26.0, 12.0, 1.6, 0.7);
    draw_stroke(&mut rgba, 26.0, 12.0, 28.0, 8.0, 1.5, 0.8);
    draw_stroke(&mut rgba, 30.0, 20.0, 28.0, 14.0, 1.4, 0.65);
    
    // Head
    draw_stroke(&mut rgba, 28.0, 8.0, 32.0, 6.0, 1.4, 0.85);
    draw_stroke(&mut rgba, 32.0, 6.0, 38.0, 8.0, 1.2, 0.9);
    draw_stroke(&mut rgba, 38.0, 8.0, 42.0, 10.0, 1.0, 0.95);
    draw_stroke(&mut rgba, 42.0, 10.0, 44.0, 12.0, 0.8, 1.0);
    draw_stroke(&mut rgba, 30.0, 10.0, 36.0, 12.0, 0.9, 0.88);
    draw_stroke(&mut rgba, 34.0, 8.0, 35.0, 9.0, 0.6, 0.0);
    draw_stroke(&mut rgba, 43.0, 11.0, 44.0, 12.0, 0.5, 0.0);
    
    // Ears
    draw_stroke(&mut rgba, 28.0, 8.0, 24.0, 2.0, 0.9, 0.9);
    draw_stroke(&mut rgba, 24.0, 2.0, 26.0, 4.0, 0.7, 0.85);
    draw_stroke(&mut rgba, 30.0, 6.0, 28.0, 0.0, 0.8, 0.92);
    draw_stroke(&mut rgba, 28.0, 0.0, 30.0, 2.0, 0.6, 0.88);
    
    // Mane
    draw_stroke(&mut rgba, 26.0, 14.0, 18.0, 8.0, 1.0, 0.5);
    draw_stroke(&mut rgba, 18.0, 8.0, 10.0, 6.0, 0.8, 0.3);
    draw_stroke(&mut rgba, 10.0, 6.0, 4.0, 8.0, 0.6, 0.1);
    draw_stroke(&mut rgba, 24.0, 16.0, 14.0, 12.0, 1.1, 0.45);
    draw_stroke(&mut rgba, 14.0, 12.0, 6.0, 12.0, 0.9, 0.2);
    draw_stroke(&mut rgba, 6.0, 12.0, 2.0, 16.0, 0.7, 0.05);
    draw_stroke(&mut rgba, 22.0, 18.0, 12.0, 16.0, 1.0, 0.4);
    draw_stroke(&mut rgba, 12.0, 16.0, 4.0, 18.0, 0.8, 0.15);
    draw_stroke(&mut rgba, 28.0, 12.0, 20.0, 6.0, 0.9, 0.55);
    draw_stroke(&mut rgba, 20.0, 6.0, 12.0, 4.0, 0.7, 0.25);
    draw_stroke(&mut rgba, 30.0, 10.0, 24.0, 4.0, 0.8, 0.6);
    draw_stroke(&mut rgba, 24.0, 4.0, 16.0, 2.0, 0.6, 0.2);
    draw_stroke(&mut rgba, 20.0, 20.0, 8.0, 20.0, 0.9, 0.35);
    draw_stroke(&mut rgba, 8.0, 20.0, 2.0, 24.0, 0.7, 0.1);
    draw_stroke(&mut rgba, 18.0, 22.0, 6.0, 24.0, 0.8, 0.25);
    draw_stroke(&mut rgba, 6.0, 24.0, 2.0, 28.0, 0.6, 0.05);
    
    (rgba, size, size)
}

/// Apply greyscale filter to RGBA data (for inactive state)
pub fn apply_greyscale(rgba: &mut [u8]) {
    for chunk in rgba.chunks_exact_mut(4) {
        let grey = ((chunk[0] as u32 + chunk[1] as u32 + chunk[2] as u32) / 3) as u8;
        chunk[0] = grey;
        chunk[1] = grey;
        chunk[2] = grey;
        // Keep alpha unchanged
    }
}

/// Create tray icon from RGBA data
pub fn rgba_to_tray_icon(rgba: Vec<u8>, width: u32, height: u32) -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon")
}

/// Create egui IconData from RGBA data (for window icons)
pub fn rgba_to_egui_icon(rgba: Vec<u8>, width: u32, height: u32) -> egui::IconData {
    egui::IconData {
        rgba,
        width,
        height,
    }
}

/// Load icon for egui window (convenience function)
pub fn load_egui_icon() -> egui::IconData {
    let (rgba, w, h) = load_icon_rgba();
    rgba_to_egui_icon(rgba, w, h)
}

/// Load active tray icon
pub fn load_tray_icon_active() -> tray_icon::Icon {
    let (rgba, w, h) = load_icon_rgba();
    rgba_to_tray_icon(rgba, w, h)
}

/// Load inactive (greyscale) tray icon
pub fn load_tray_icon_inactive() -> tray_icon::Icon {
    let (mut rgba, w, h) = load_icon_rgba();
    apply_greyscale(&mut rgba);
    rgba_to_tray_icon(rgba, w, h)
}

// =============================================================================
// 7. FRAME BUILDERS  (single source of truth for component shells)
// =============================================================================
//
// Instead of every call site hand-crafting `egui::Frame::default().fill(...)
// .stroke(...).rounding(...).inner_margin(...)` with ad-hoc numbers, components
// pull a pre-tuned frame here. This is what gives the UI a coherent "texture":
// consistent radius, stroke, padding and surface elevation everywhere.
//
// Painting depth:
//   `shadow_*` helpers add a soft drop shadow behind a frame. egui has no
//   native blur, so we emulate it with a 1px translucent ring drawn just
//   outside the frame rect (cheap, looks good on dark backgrounds).

/// Inner margin helper — symmetric on all sides.
fn pad(all: f32) -> egui::Margin {
    egui::Margin::same(all)
}

/// Top-level panel frame (e.g. side navigation, bottom action bar).
/// Flat against the root surface, hairline border, no rounding.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(SURFACE_1)
        .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
        .rounding(egui::Rounding::same(0.0))
        .inner_margin(pad(SPACE_6))
}

/// In-panel card / section container. Lifts off the panel (SURFACE_2),
/// rounded, hairline border.
pub fn card_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(SURFACE_2)
        .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
        .rounding(egui::Rounding::same(RADIUS_LG))
        .inner_margin(pad(SPACE_5))
}

/// Floating popup / modal / result window. Highest elevation (SURFACE_3),
/// accent ring, generous padding, large radius.
pub fn popup_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(SURFACE_3)
        .stroke(egui::Stroke::new(STROKE_ACCENT, ACCENT_BORDER))
        .rounding(egui::Rounding::same(RADIUS_XL))
        .inner_margin(pad(SPACE_7))
}

/// Compact indicator frame (e.g. the Waiting-state processing bar).
/// Pill-ish: small radius, accent ring, tight padding.
pub fn indicator_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(SURFACE_3)
        .stroke(egui::Stroke::new(STROKE_ACCENT, ACCENT_BORDER))
        .rounding(egui::Rounding::same(RADIUS_LG))
        .inner_margin(pad(SPACE_4))
}

/// Paint a soft drop shadow ring around `rect` to give a floating frame depth.
/// `ring_px` is how far the shadow extends beyond the rect; `alpha` is peak
/// opacity at the rect edge (fades outward). Cheap glow emulation.
pub fn paint_shadow_ring(painter: &egui::Painter, rect: egui::Rect, ring_px: f32, alpha: u8) {
    if ring_px <= 0.0 || alpha == 0 {
        return;
    }
    // Draw 3 concentric expanded rects with decreasing alpha for a soft falloff.
    let steps = 3;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let expand = ring_px * t;
        let a = ((alpha as f32) * (1.0 - t) * 0.5) as u8;
        if a == 0 {
            continue;
        }
        let expanded = rect.expand(expand);
        painter.rect_filled(
            expanded,
            egui::Rounding::same(RADIUS_XL + expand),
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, a),
        );
    }
}

/// Paint a subtle accent glow ring (cyan) around `rect`. Used to make popups
/// and selected cards feel "energised" without a hard neon edge.
pub fn paint_accent_glow(painter: &egui::Painter, rect: egui::Rect, ring_px: f32) {
    if ring_px <= 0.0 {
        return;
    }
    let steps = 3;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let expand = ring_px * t;
        // Cyan glow: low alpha, fades outward.
        let a = ((1.0 - t) * 36.0) as u8;
        if a == 0 {
            continue;
        }
        let expanded = rect.expand(expand);
        painter.rect_filled(
            expanded,
            egui::Rounding::same(RADIUS_XL + expand),
            egui::Color32::from_rgba_unmultiplied(0, 200, 230, a),
        );
    }
}

/// Standard control-height scales (button heights, row heights).
pub const CTRL_H_SM: f32 = 24.0;
pub const CTRL_H_MD: f32 = 30.0;
pub const CTRL_H_LG: f32 = 36.0;

/// Apply the Japan 2046 cyberpunk theme to an egui context
pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // Surfaces
    visuals.panel_fill = SURFACE_1;
    visuals.window_fill = SURFACE_0;
    visuals.extreme_bg_color = SURFACE_0;
    visuals.faint_bg_color = SURFACE_2;
    visuals.override_text_color = Some(TEXT_COLOR);

    // Selection — softened so it does not flood the UI with raw neon pink.
    visuals.selection.bg_fill = ACCENT_SELECT;
    visuals.selection.stroke = egui::Stroke::new(STROKE_HAIRLINE, egui::Color32::WHITE);

    // Widgets — use elevation surfaces, not raw neon, for default states.
    visuals.widgets.noninteractive.bg_fill = SURFACE_1;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(STROKE_HAIRLINE, TEXT_MUTED);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(STROKE_HAIRLINE, BORDER);

    visuals.widgets.inactive.bg_fill = SURFACE_2;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(STROKE_HAIRLINE, TEXT_COLOR);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(STROKE_HAIRLINE, BORDER);

    visuals.widgets.hovered.bg_fill = HOVERED_BG;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(STROKE_HAIRLINE, NEON_CYAN);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(STROKE_THIN, ACCENT_BORDER);

    visuals.widgets.active.bg_fill = NEON_CYAN;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(STROKE_HAIRLINE, egui::Color32::BLACK);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(STROKE_ACCENT, NEON_CYAN);

    visuals.widgets.open.bg_fill = SURFACE_2;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(STROKE_HAIRLINE, NEON_CYAN);

    // Window styling — rounded, accent ring, soft shadow.
    visuals.window_rounding = egui::Rounding::same(RADIUS_XL);
    visuals.window_stroke = egui::Stroke::new(STROKE_THIN, BORDER);
    visuals.menu_rounding = egui::Rounding::same(RADIUS_LG);
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.inactive.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.hovered.rounding = egui::Rounding::same(RADIUS_MD);
    visuals.widgets.active.rounding = egui::Rounding::same(RADIUS_MD);

    ctx.set_visuals(visuals);
}

/// Configure CJK fonts for proper Chinese/Japanese/Korean text rendering
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // Cross-platform font search list
    let font_candidates = [
        // Windows
        "c:\\Windows\\Fonts\\msyh.ttc",    // Microsoft YaHei
        "c:\\Windows\\Fonts\\simhei.ttf",   // SimHei
        "c:\\Windows\\Fonts\\msgothic.ttc", // MS Gothic
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    
    for path in font_candidates {
        if let Ok(font_data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk_font".to_owned(),
                egui::FontData::from_owned(font_data),
            );

            // Put CJK font first (highest priority) for proportional text
            fonts.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk_font".to_owned());

            // Put CJK font as last fallback for monospace
            fonts.families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk_font".to_owned());

            ctx.set_fonts(fonts);
            info!("Loaded CJK font from {}", path);
            return;
        }
    }
    log::warn!("Failed to load any CJK fonts. Characters may not display correctly.");
}
