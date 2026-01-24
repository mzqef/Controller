use crate::core::memory::*;
use crate::core::memory_store::MemoryStore;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

const NODE_RADIUS: f32 = 35.0;
const NODE_SPACING_Y: f32 = 90.0;
const COLUMN_SPACING: f32 = 250.0;
const DEFAULT_LIMIT_PER_TIER: usize = 10;

/// Ghost node for dangling edge sources (items lost on reboot)
#[derive(Clone)]
struct GhostNode {
    id: Uuid,
    position: Pos2,
}

pub struct MemoryGraphView {
    store: Arc<MemoryStore>,
    // Cached data
    cached_items: Vec<MemoryItem>,
    cached_edges: Vec<MemoryEdge>,
    ghost_nodes: HashMap<Uuid, GhostNode>,
    last_revision: u64,
    // Per-tier limits
    short_term_limit: usize,
    mid_term_limit: usize,
    long_term_limit: usize,
    // Layout
    node_positions: HashMap<Uuid, Pos2>,
    // UI state
    selected_node: Option<Uuid>,
    hovered_node: Option<Uuid>,
    dragging_node: Option<Uuid>,
    drag_offset: Vec2,
    edge_creation_source: Option<Uuid>,
    view_offset: Vec2,
    zoom: f32,
    show_short_term: bool,
    show_mid_term: bool,
    show_long_term: bool,
    search_query: String,
    // Title editing state (persist across frames)
    title_edit_id: Option<Uuid>,
    title_edit_buffer: String,
}

impl MemoryGraphView {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self {
            store,
            cached_items: Vec::new(),
            cached_edges: Vec::new(),
            ghost_nodes: HashMap::new(),
            last_revision: 0,
            short_term_limit: DEFAULT_LIMIT_PER_TIER,
            mid_term_limit: DEFAULT_LIMIT_PER_TIER,
            long_term_limit: DEFAULT_LIMIT_PER_TIER,
            node_positions: HashMap::new(),
            selected_node: None,
            hovered_node: None,
            dragging_node: None,
            drag_offset: Vec2::ZERO,
            edge_creation_source: None,
            view_offset: Vec2::ZERO,
            zoom: 1.0,
            show_short_term: true,
            show_mid_term: true,
            show_long_term: true,
            search_query: String::new(),
            title_edit_id: None,
            title_edit_buffer: String::new(),
        }
    }

    /// Refresh cache if revision changed
    fn refresh_cache_if_needed(&mut self) {
        let current_rev = self.store.get_revision();
        if current_rev == self.last_revision && !self.cached_items.is_empty() {
            return;
        }
        self.last_revision = current_rev;

        // Load items by tier with limits
        let short_items = self.store.list_items_by_type(MemoryType::ShortTerm, self.short_term_limit, 0);
        let mid_items = self.store.list_items_by_type(MemoryType::MidTerm, self.mid_term_limit, 0);
        let long_items = self.store.list_items_by_type(MemoryType::LongTerm, self.long_term_limit, 0);

        self.cached_items = short_items.into_iter()
            .chain(mid_items)
            .chain(long_items)
            .collect();

        // Get edges for visible items
        let item_ids: HashSet<Uuid> = self.cached_items.iter().map(|i| i.id).collect();
        self.cached_edges = self.store.list_edges_for_items(&item_ids);

        // Find ghost nodes (edges referencing missing sources)
        self.ghost_nodes.clear();
        for edge in &self.cached_edges {
            if !item_ids.contains(&edge.source_id) && !self.store.item_exists(edge.source_id) {
                // This is a dangling edge - source is gone
                if !self.ghost_nodes.contains_key(&edge.source_id) {
                    self.ghost_nodes.insert(edge.source_id, GhostNode {
                        id: edge.source_id,
                        position: Pos2::new(-300.0, 0.0), // Will be positioned in layout
                    });
                }
            }
        }

        // Prune stale node positions
        let valid_ids: HashSet<Uuid> = item_ids.iter().copied()
            .chain(self.ghost_nodes.keys().copied())
            .collect();
        self.node_positions.retain(|id, _| valid_ids.contains(id));

        // Recompute layout
        self.reflow_layout();
    }

    /// Recompute tier-stacked layout (short left, mid center, long right)
    fn reflow_layout(&mut self) {
        // Column X positions
        let x_short = -COLUMN_SPACING;
        let x_mid = 0.0;
        let x_long = COLUMN_SPACING;

        // Group items by tier
        let mut short_items: Vec<_> = self.cached_items.iter()
            .filter(|i| i.memory_type == MemoryType::ShortTerm)
            .collect();
        let mut mid_items: Vec<_> = self.cached_items.iter()
            .filter(|i| i.memory_type == MemoryType::MidTerm)
            .collect();
        let mut long_items: Vec<_> = self.cached_items.iter()
            .filter(|i| i.memory_type == MemoryType::LongTerm)
            .collect();

        // Sort by recency (newest first for stacking)
        short_items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        mid_items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        long_items.sort_by(|a, b| {
            let a_time = a.promoted_at.unwrap_or(a.created_at);
            let b_time = b.promoted_at.unwrap_or(b.created_at);
            b_time.cmp(&a_time)
        });

        // Position nodes (newest at top, y=0)
        // If an item has a persisted position, honor it.
        // Otherwise, keep an existing in-session position if present (e.g., dragged short-term nodes).
        for (idx, item) in short_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_short, default_y);
            let new_pos = item
                .position
                .map(|(x, y)| Pos2::new(x, y))
                .or_else(|| self.node_positions.get(&item.id).copied())
                .unwrap_or(default_pos);
            self.node_positions.insert(item.id, new_pos);
        }
        for (idx, item) in mid_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_mid, default_y);
            let new_pos = item
                .position
                .map(|(x, y)| Pos2::new(x, y))
                .or_else(|| self.node_positions.get(&item.id).copied())
                .unwrap_or(default_pos);
            self.node_positions.insert(item.id, new_pos);
        }
        for (idx, item) in long_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_long, default_y);
            let new_pos = item
                .position
                .map(|(x, y)| Pos2::new(x, y))
                .or_else(|| self.node_positions.get(&item.id).copied())
                .unwrap_or(default_pos);
            self.node_positions.insert(item.id, new_pos);
        }

        // Position ghost nodes in a separate area
        for (idx, ghost) in self.ghost_nodes.values_mut().enumerate() {
            ghost.position = Pos2::new(-COLUMN_SPACING * 1.5, idx as f32 * NODE_SPACING_Y);
            self.node_positions.insert(ghost.id, ghost.position);
        }
    }

    fn get_node_color(&self, mem_type: MemoryType) -> Color32 {
        match mem_type {
            MemoryType::ShortTerm => Color32::from_rgb(0, 200, 255),
            MemoryType::MidTerm => Color32::from_rgb(255, 200, 0),
            MemoryType::LongTerm => Color32::from_rgb(0, 255, 100),
        }
    }

    fn get_edge_color(&self, relation: RelationType) -> Color32 {
        match relation {
            RelationType::TranslatedTo => Color32::from_rgb(255, 100, 255),
            RelationType::ExplainedBy => Color32::from_rgb(100, 200, 255),
            RelationType::FormattedTo => Color32::from_rgb(255, 150, 100),
            RelationType::DerivedFrom => Color32::from_rgb(200, 200, 200),
            RelationType::PromotedFrom => Color32::from_rgb(0, 255, 100),
            RelationType::UserLinked => Color32::from_rgb(255, 255, 255),
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Refresh cache if store revision changed
        self.refresh_cache_if_needed();

        // Top toolbar
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_short_term, "📋 Short-term");
            ui.checkbox(&mut self.show_mid_term, "🔄 Mid-term");
            ui.checkbox(&mut self.show_long_term, "💾 Long-term");
            ui.separator();
            
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.search_query)
                .desired_width(150.0)
                .hint_text("Search..."));
            
            ui.separator();
            
            if ui.button("Auto Align").clicked() {
                self.reflow_layout();
            }
            
            if self.edge_creation_source.is_some() {
                ui.colored_label(Color32::YELLOW, "Click target node (ESC to cancel)");
            }
        });

        ui.separator();

        // Legend with ghost indicator
        ui.horizontal(|ui| {
            ui.label("Legend:");
            ui.colored_label(Color32::from_rgb(0, 200, 255), "● Short-term");
            ui.colored_label(Color32::from_rgb(255, 200, 0), "● Mid-term");
            ui.colored_label(Color32::from_rgb(0, 255, 100), "● Long-term");
            ui.colored_label(Color32::from_rgb(100, 100, 100), "◌ Ghost");
            ui.separator();
            ui.colored_label(Color32::from_rgb(255, 100, 255), "→ Translated");
            ui.colored_label(Color32::from_rgb(100, 200, 255), "→ Explained");
            ui.colored_label(Color32::from_rgb(0, 255, 100), "→ Promoted");
        });

        // Load More buttons per tier
        ui.horizontal(|ui| {
            let short_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::ShortTerm).count();
            let mid_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::MidTerm).count();
            let long_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::LongTerm).count();
            
            ui.label(format!("📋 Short: {}", short_count));
            if ui.small_button("+10").clicked() {
                self.short_term_limit += 10;
                self.last_revision = 0; // Force refresh
            }
            ui.separator();
            
            ui.label(format!("🔄 Mid: {}", mid_count));
            if ui.small_button("+10").clicked() {
                self.mid_term_limit += 10;
                self.last_revision = 0;
            }
            ui.separator();
            
            ui.label(format!("💾 Long: {}", long_count));
            if ui.small_button("+10").clicked() {
                self.long_term_limit += 10;
                self.last_revision = 0;
            }
        });

        ui.separator();

        // Main graph area
        let available_size = ui.available_size();
        let (response, painter) = ui.allocate_painter(available_size, Sense::click_and_drag());
        let rect = response.rect;

        // Handle panning
        if response.dragged() && self.dragging_node.is_none() {
            self.view_offset += response.drag_delta();
        }

        // Handle zoom
        let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_delta != 0.0 && rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
            self.zoom = (self.zoom * (1.0 + scroll_delta * 0.001)).clamp(0.3, 2.5);
        }

        // Background
        painter.rect_filled(rect, 0.0, Color32::from_rgb(15, 15, 25));

        // Grid pattern
        let grid_spacing = 50.0 * self.zoom;
        let grid_color = Color32::from_rgba_unmultiplied(50, 50, 70, 100);
        let offset_x = self.view_offset.x % grid_spacing;
        let offset_y = self.view_offset.y % grid_spacing;
        
        let mut x = rect.left() + offset_x;
        while x < rect.right() {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(0.5, grid_color),
            );
            x += grid_spacing;
        }
        let mut y = rect.top() + offset_y;
        while y < rect.bottom() {
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(0.5, grid_color),
            );
            y += grid_spacing;
        }

        // Use cached items and edges
        let search_lower = self.search_query.to_lowercase();
        let visible_items: Vec<_> = self.cached_items
            .iter()
            .filter(|i| match i.memory_type {
                MemoryType::ShortTerm => self.show_short_term,
                MemoryType::MidTerm => self.show_mid_term,
                MemoryType::LongTerm => self.show_long_term,
            })
            .filter(|i| {
                self.search_query.is_empty() || 
                i.content.to_lowercase().contains(&search_lower)
            })
            .collect();

        // Draw edges (including those from ghost nodes)
        let visible_ids: HashSet<_> = visible_items.iter().map(|i| i.id).collect();
        let ghost_ids: HashSet<_> = self.ghost_nodes.keys().copied().collect();
        for edge in &self.cached_edges {
            let source_visible = visible_ids.contains(&edge.source_id) || ghost_ids.contains(&edge.source_id);
            let target_visible = visible_ids.contains(&edge.target_id);
            if !source_visible || !target_visible {
                continue;
            }

            if let (Some(&from_pos), Some(&to_pos)) = (
                self.node_positions.get(&edge.source_id),
                self.node_positions.get(&edge.target_id),
            ) {
                let from_screen = self.world_to_screen(from_pos, rect);
                let to_screen = self.world_to_screen(to_pos, rect);

                // Skip if off-screen
                if !rect.expand(50.0).contains(from_screen) && !rect.expand(50.0).contains(to_screen) {
                    continue;
                }

                // Draw a short stub (no arrowheads). If nodes overlap, use a default direction.
                let delta = to_screen - from_screen;
                let len_sq = delta.length_sq();
                let dir = if len_sq > 1.0e-6 { delta / len_sq.sqrt() } else { Vec2::X };

                let node_radius = NODE_RADIUS * self.zoom;
                let start = from_screen + dir * node_radius;
                let stub_len = (18.0 * self.zoom).max(6.0);
                let end = start + dir * stub_len;

                let color = self.get_edge_color(edge.relation);
                painter.line_segment([start, end], Stroke::new(2.0 * self.zoom, color));
            }
        }

        // Edge creation preview line
        if let Some(source_id) = self.edge_creation_source {
            if let Some(&source_pos) = self.node_positions.get(&source_id) {
                let from_screen = self.world_to_screen(source_pos, rect);
                if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                    painter.line_segment(
                        [from_screen, pointer],
                        Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                    );
                }
            }
        }

        // Draw nodes
        self.hovered_node = None;
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());

        for item in &visible_items {
            if let Some(&world_pos) = self.node_positions.get(&item.id) {
                let screen_pos = self.world_to_screen(world_pos, rect);
                
                // Skip if off-screen
                if !rect.expand(NODE_RADIUS * self.zoom).contains(screen_pos) {
                    continue;
                }
                
                let node_radius = NODE_RADIUS * self.zoom;

                // Check hover
                let is_hovered = pointer_pos
                    .map(|p| (p - screen_pos).length() < node_radius)
                    .unwrap_or(false);
                if is_hovered {
                    self.hovered_node = Some(item.id);
                }

                let is_selected = self.selected_node == Some(item.id);
                let is_edge_source = self.edge_creation_source == Some(item.id);
                let base_color = self.get_node_color(item.memory_type);

                // Node circle
                let fill_color = if is_edge_source {
                    Color32::YELLOW
                } else if is_selected {
                    Color32::WHITE
                } else if is_hovered {
                    base_color.gamma_multiply(1.3)
                } else {
                    base_color
                };

                // Shadow
                painter.circle_filled(
                    screen_pos + Vec2::new(3.0, 3.0),
                    node_radius,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 80),
                );
                
                painter.circle_filled(screen_pos, node_radius, fill_color);
                painter.circle_stroke(screen_pos, node_radius, Stroke::new(2.0, Color32::WHITE));

                // Label: prefer title, otherwise preview (truncate to fit)
                let text_source = item.title.as_deref().filter(|s| !s.trim().is_empty()).map(|s| s.to_string()).unwrap_or_else(|| item.preview.clone());
                let truncated: String = text_source.chars().take(15).collect();
                let label = if truncated.len() < text_source.len() {
                    format!("{}...", truncated)
                } else {
                    truncated
                };
                painter.text(
                    screen_pos,
                    egui::Align2::CENTER_CENTER,
                    &label,
                    FontId::proportional(11.0 * self.zoom),
                    Color32::BLACK,
                );
            }
        }

        // Draw ghost nodes (lost on reboot)
        for ghost in self.ghost_nodes.values() {
            let screen_pos = self.world_to_screen(ghost.position, rect);
            if !rect.expand(NODE_RADIUS * self.zoom).contains(screen_pos) {
                continue;
            }
            let node_radius = NODE_RADIUS * self.zoom * 0.7;
            let ghost_color = Color32::from_rgb(80, 80, 80);
            
            // Dashed circle effect
            painter.circle_stroke(screen_pos, node_radius, Stroke::new(2.0, ghost_color));
            painter.text(
                screen_pos,
                egui::Align2::CENTER_CENTER,
                "👻",
                FontId::proportional(14.0 * self.zoom),
                ghost_color,
            );
        }

        // Handle interactions
        if response.clicked() {
            if let Some(hovered) = self.hovered_node {
                if let Some(source) = self.edge_creation_source {
                    if source != hovered {
                        self.store.add_user_edge(source, hovered);
                    }
                    self.edge_creation_source = None;
                } else {
                    self.selected_node = Some(hovered);
                }
            } else {
                self.selected_node = None;
                self.edge_creation_source = None;
            }
        }

        if response.drag_started() {
            if let Some(hovered) = self.hovered_node {
                self.dragging_node = Some(hovered);
                if let Some(&pos) = self.node_positions.get(&hovered) {
                    let screen_pos = self.world_to_screen(pos, rect);
                    self.drag_offset = pointer_pos.unwrap_or(screen_pos) - screen_pos;
                }
            }
        }

        if response.dragged() {
            if let Some(dragging) = self.dragging_node {
                if let Some(pointer) = pointer_pos {
                    let new_screen_pos = pointer - self.drag_offset;
                    let new_world_pos = self.screen_to_world(new_screen_pos, rect);
                    self.node_positions.insert(dragging, new_world_pos);
                }
            }
        }

        if response.drag_stopped() {
            // Persist node position only when drag ends.
            if let Some(dragging) = self.dragging_node.take() {
                if self.store.item_exists(dragging) {
                    if let Some(&pos) = self.node_positions.get(&dragging) {
                        self.store.update_item_position(dragging, pos.x, pos.y);
                    }
                }
            }
        }

        // ESC to cancel edge creation
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.edge_creation_source = None;
            self.selected_node = None;
        }

        // Selected node details panel
        if let Some(selected_id) = self.selected_node {
            if let Some(item) = self.store.get_item(selected_id) {
                self.draw_details_panel(ui, &item);
            }
        }

        // Request repaint for smooth animation
        if self.dragging_node.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else {
            ctx.request_repaint();
        }
    }

    fn draw_details_panel(&mut self, ui: &mut egui::Ui, item: &MemoryItem) {
        egui::Window::new("📝 Memory Details")
            .default_width(350.0)
            .anchor(egui::Align2::RIGHT_TOP, [-10.0, 40.0])
            .show(ui.ctx(), |ui| {
                let type_str = match item.memory_type {
                    MemoryType::ShortTerm => "📋 Short-term (Clipboard)",
                    MemoryType::MidTerm => "🔄 Mid-term (Processed)",
                    MemoryType::LongTerm => "💾 Long-term (Persisted)",
                };
                ui.label(type_str);
                ui.label(format!("Created: {}", item.created_at.format("%Y-%m-%d %H:%M:%S")));
                if let Some(promoted) = item.promoted_at {
                    ui.label(format!("Promoted: {}", promoted.format("%Y-%m-%d %H:%M:%S")));
                }

                // Show metadata
                match &item.metadata {
                    MemoryMetadata::ActionResult { action_type, .. } => {
                        let action_str = match action_type {
                            ActionType::Format => "Copy Check / Format",
                            ActionType::TranslateE2C => "Translation (EN→中)",
                            ActionType::TranslateC2E => "Translation (中→EN)",
                            ActionType::Explain => "Explanation",
                            ActionType::UserQuery => "User Query",
                        };
                        ui.label(format!("Action: {}", action_str));
                    }
                    _ => {}
                }

                ui.separator();
                // Editable title
                if self.title_edit_id != Some(item.id) {
                    self.title_edit_id = Some(item.id);
                    self.title_edit_buffer = item.title.clone().unwrap_or_default();
                }

                ui.horizontal(|ui| {
                    ui.label("Title:");
                    let title_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.title_edit_buffer)
                            .desired_width(240.0)
                            .hint_text("Optional title (Enter to save)"),
                    );

                    // Save on Enter only.
                    if title_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.store.update_item_title(item.id, self.title_edit_buffer.clone());
                        ui.ctx().memory_mut(|mem| mem.surrender_focus(title_resp.id));
                    }
                });

                ui.label("Content:");
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut item.content.clone())
                            .desired_width(f32::INFINITY)
                            .interactive(false));
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if item.memory_type != MemoryType::MidTerm {
                        if ui.button("→ Mid").clicked() {
                            if let Ok(new_id) = self.store.clone_promote_item(item.id, MemoryType::MidTerm) {
                                self.selected_node = Some(new_id);
                            }
                        }
                    }
                    if item.memory_type != MemoryType::LongTerm {
                        if ui.button("→ Long").clicked() {
                            if let Ok(new_id) = self.store.clone_promote_item(item.id, MemoryType::LongTerm) {
                                self.selected_node = Some(new_id);
                            }
                        }
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("🔗 Link to...").clicked() {
                        self.edge_creation_source = Some(item.id);
                    }
                    if ui.button("📋 Copy").clicked() {
                        ui.ctx().copy_text(item.content.clone());
                    }
                    if ui.button("🗑 Delete").clicked() {
                        let _ = self.store.delete_item(item.id);
                        self.selected_node = None;
                    }
                });
            });
    }

    fn reset_layout(&mut self) {
        self.node_positions.clear();
        self.view_offset = Vec2::ZERO;
        self.zoom = 1.0;
        self.short_term_limit = DEFAULT_LIMIT_PER_TIER;
        self.mid_term_limit = DEFAULT_LIMIT_PER_TIER;
        self.long_term_limit = DEFAULT_LIMIT_PER_TIER;
        self.last_revision = 0; // Force cache refresh
    }

    fn world_to_screen(&self, world_pos: Pos2, rect: Rect) -> Pos2 {
        let centered = world_pos.to_vec2() * self.zoom;
        rect.center() + centered + self.view_offset
    }

    fn screen_to_world(&self, screen_pos: Pos2, rect: Rect) -> Pos2 {
        let offset = screen_pos - rect.center() - self.view_offset;
        (offset / self.zoom).to_pos2()
    }
}
