use crate::core::memory::*;
// use crate::core::memory_store::MemoryStore; // REMOVE
use crate::core::ipc_messages::GraphRequest;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// Design tokens (see src/ui/theme.rs). The graph canvas is custom-painted, so
// we pull the canonical type/spacing scale here to keep the toolbar and labels
// in lock-step with the rest of the app.
use crate::ui::theme::{
    SPACE_4,
    TEXT_XS, TEXT_SM,
};

const NODE_RADIUS: f32 = 38.0;       // was 35 — slightly larger for better label breathing room
const NODE_SPACING_Y: f32 = 96.0;    // was 90 — matches new radius + padding
const COLUMN_SPACING: f32 = 260.0;   // was 250 — matches new node diameter + gap
const DEFAULT_LIMIT_PER_TIER: usize = 10;

/// Ghost node for dangling edge sources (items lost on reboot)
#[derive(Clone)]
struct GhostNode {
    id: Uuid,
    position: Pos2,
}

pub struct MemoryGraphView {
    // store: Arc<MemoryStore>, // REMOVE
    // Cached data
    cached_items: Vec<MemoryItem>,
    cached_edges: Vec<MemoryEdge>,
    ghost_nodes: HashMap<Uuid, GhostNode>,
    // last_revision: u64, // REMOVE
    
    // Per-tier limits (client-side filter only, or maybe request params later)
    short_term_limit: usize,
    mid_term_limit: usize,
    long_term_limit: usize,
    // Layout
    node_positions: HashMap<Uuid, Pos2>,
    // UI state
    selected_nodes: HashSet<Uuid>,
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
    title_edit_dirty: bool,
    
    // Marquee (Shift+Drag) selection state
    marquee_start: Option<Pos2>,
    marquee_current: Option<Pos2>,
    
    // Auto-align flag: when true, ignore persisted positions on next reflow
    force_default_layout: bool,
    
    // Export feedback for user (time, message, is_success)
    export_feedback: Option<(std::time::Instant, String, bool)>,
    
    // Generic operation feedback (Auto Connect / Clear), reuses the same
    // (time, message, is_success) shape as export_feedback.
    op_feedback: Option<(std::time::Instant, String, bool)>,
    
    // True while an Auto Connect request is in flight (disables the button).
    auto_connect_pending: bool,
    // When the in-flight Auto Connect request started, for a safety timeout.
    auto_connect_started_at: Option<std::time::Instant>,

    // Two-step confirmation for the Clear (trash) button. When true, the next
    // click within `confirm_clear_at` + 3s actually clears the graph.
    confirm_clear: bool,
    confirm_clear_at: std::time::Instant,
    
    // Configured export path (from config)
    export_path: Option<String>,
    
    // IPC
    mutations: Vec<GraphRequest>,
}

impl MemoryGraphView {
    pub fn new() -> Self {
        Self::new_with_export_path(None)
    }
    
    pub fn new_with_export_path(export_path: Option<String>) -> Self {
        Self {
            cached_items: Vec::new(),
            cached_edges: Vec::new(),
            ghost_nodes: HashMap::new(),
            short_term_limit: DEFAULT_LIMIT_PER_TIER,
            mid_term_limit: DEFAULT_LIMIT_PER_TIER,
            long_term_limit: DEFAULT_LIMIT_PER_TIER,
            node_positions: HashMap::new(),
            selected_nodes: HashSet::new(),
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
            title_edit_dirty: false,
            marquee_start: None,
            marquee_current: None,
            force_default_layout: false,
            export_feedback: None,
            op_feedback: None,
            auto_connect_pending: false,
            auto_connect_started_at: None,
            confirm_clear: false,
            confirm_clear_at: std::time::Instant::now(),
            export_path,
            mutations: Vec::new(),
        }
    }

    pub fn set_data(&mut self, items: Vec<MemoryItem>, edges: Vec<MemoryEdge>) {
        // Debug: log items with titles
        for item in &items {
            if item.title.is_some() {
                log::debug!("set_data: received item {} with title {:?}", item.id, item.title);
            }
        }
        
        let item_ids: std::collections::HashSet<_> = items.iter().map(|i| i.id).collect();
        self.cached_items = items;
        // Filter out edges with missing endpoints to prevent crashes
        self.cached_edges = edges
            .into_iter()
            .filter(|e| item_ids.contains(&e.source_id) && item_ids.contains(&e.target_id))
            .collect();
        self.refresh_ghost_nodes();
        self.reflow_layout(); // Recalculate layout with new data

        // A fresh snapshot arrived. If an Auto Connect request was in flight,
        // the AI-derived edges are now baked into this snapshot, so we can
        // clear the pending flag and surface a completion message. Without
        // this, the button would re-enable on the next frame (mutations drained)
        // long before the LLM responded, so the user could not tell it was
        // still running.
        if self.auto_connect_pending {
            self.auto_connect_pending = false;
            self.auto_connect_started_at = None;
            self.op_feedback = Some((std::time::Instant::now(), "Auto Connect done".to_string(), true));
        }
    }

    pub fn drain_mutations(&mut self) -> Vec<GraphRequest> {
        self.mutations.drain(..).collect()
    }

    fn refresh_ghost_nodes(&mut self) {
         let item_ids: HashSet<Uuid> = self.cached_items.iter().map(|i| i.id).collect();
         self.ghost_nodes.clear();
         for edge in &self.cached_edges {
             // Handle missing source
             if !item_ids.contains(&edge.source_id) {
                 if !self.ghost_nodes.contains_key(&edge.source_id) {
                     self.ghost_nodes.insert(edge.source_id, GhostNode {
                         id: edge.source_id,
                         position: Pos2::new(-300.0, 0.0),
                     });
                 }
             }
             // Handle missing target
             if !item_ids.contains(&edge.target_id) {
                 if !self.ghost_nodes.contains_key(&edge.target_id) {
                     self.ghost_nodes.insert(edge.target_id, GhostNode {
                         id: edge.target_id,
                         position: Pos2::new(-300.0, 50.0),
                     });
                 }
             }
         }
         
        // Prune stale node positions
        let valid_ids: HashSet<Uuid> = item_ids.iter().copied()
            .chain(self.ghost_nodes.keys().copied())
            .collect();
        self.node_positions.retain(|id, _| valid_ids.contains(id));
    }
    
    // REMOVED refresh_cache_if_needed

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
        // If force_default_layout is set, ignore persisted positions.
        // Otherwise, honor item.position from server, then session positions, then default.
        let ignore_persisted = self.force_default_layout;
        
        for (idx, item) in short_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_short, default_y);
            let new_pos = if ignore_persisted {
                default_pos
            } else {
                item.position
                    .map(|(x, y)| Pos2::new(x, y))
                    .or_else(|| self.node_positions.get(&item.id).copied())
                    .unwrap_or(default_pos)
            };
            self.node_positions.insert(item.id, new_pos);
        }
        for (idx, item) in mid_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_mid, default_y);
            let new_pos = if ignore_persisted {
                default_pos
            } else {
                item.position
                    .map(|(x, y)| Pos2::new(x, y))
                    .or_else(|| self.node_positions.get(&item.id).copied())
                    .unwrap_or(default_pos)
            };
            self.node_positions.insert(item.id, new_pos);
        }
        for (idx, item) in long_items.iter().enumerate() {
            let default_y = idx as f32 * NODE_SPACING_Y;
            let default_pos = Pos2::new(x_long, default_y);
            let new_pos = if ignore_persisted {
                default_pos
            } else {
                item.position
                    .map(|(x, y)| Pos2::new(x, y))
                    .or_else(|| self.node_positions.get(&item.id).copied())
                    .unwrap_or(default_pos)
            };
            self.node_positions.insert(item.id, new_pos);
        }
        
        // Reset the flag after applying
        self.force_default_layout = false;

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

    // Truncate a label by Unicode scalar characters (works reasonably for CJK)
    fn truncate_label(s: &str, max_chars: usize) -> String {
        let count = s.chars().count();
        if count <= max_chars {
            s.to_string()
        } else {
            let truncated: String = s.chars().take(max_chars).collect();
            format!("{}…", truncated)
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Cache refresh is now driven by external data setter
       
        // Top toolbar — styled to match the Functions Config window: consistent
        // item spacing (SPACE_4), token-typed buttons, and feedback pills.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_4;
            ui.checkbox(&mut self.show_short_term, "📋 Short-term");
            ui.checkbox(&mut self.show_mid_term, "🔄 Mid-term");
            ui.checkbox(&mut self.show_long_term, "💾 Long-term");
            ui.separator();
            
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.search_query)
                .desired_width(180.0)
                .hint_text("Search..."));
            
            ui.separator();
            
            if ui.button("Auto Align")
                .on_hover_text("Reset all node positions to the default three-column tier layout.") 
                .clicked() {
                // Set flag to ignore persisted positions on next reflow
                self.force_default_layout = true;
                self.node_positions.clear();
                // Clear server-side positions so they don't overwrite on next snapshot
                self.mutations.push(GraphRequest::ClearAllPositions);
                self.reflow_layout();
                self.op_feedback = Some((std::time::Instant::now(), "Realigned".to_string(), true));
            }
            // Auto Connect — ask the AI to link related but unconnected nodes.
            // Disabled while a request is in flight so the user can see it is
            // still running (the flag is cleared in set_data when the refreshed
            // snapshot arrives).
            let connect_enabled = !self.auto_connect_pending && !self.cached_items.is_empty();
            let connect_label = if self.auto_connect_pending {
                "Auto Connect…" // ellipsis signals in-progress
            } else {
                "Auto Connect"
            };
            if ui.add_enabled(connect_enabled, egui::Button::new(connect_label))
                .on_hover_text("Ask the AI to find related items and draw edges between them. Runs the \"connect\" action.") 
                .clicked() {
                self.auto_connect_pending = true;
                self.auto_connect_started_at = Some(std::time::Instant::now());
                self.mutations.push(GraphRequest::AutoConnectEdges);
                self.op_feedback = Some((std::time::Instant::now(), "Connecting…".to_string(), true));
            }
            // Persistent in-progress indicator: stays visible until the snapshot
            // returns (the 3-second fading pill alone was too brief to convey a
            // long-running LLM call).
            if self.auto_connect_pending {
                ui.colored_label(Color32::from_rgb(0, 200, 230), "Connecting…");
                // Keep repainting so the spinner/label stays live.
                ctx.request_repaint();
            }
            // Trash — clear the entire graph (items + edges). Two-step confirm
            // (click Clear, then click again within 3s) to avoid data loss.
            if ui.button("Clear")
                .on_hover_text("Remove ALL items and edges from the graph (memory + storage). Click twice to confirm.") 
                .clicked() {
                if self.confirm_clear {
                    self.mutations.push(GraphRequest::ClearGraph);
                    self.selected_nodes.clear();
                    self.node_positions.clear();
                    self.op_feedback = Some((std::time::Instant::now(), "Graph cleared".to_string(), true));
                    self.confirm_clear = false;
                } else {
                    self.confirm_clear = true;
                    self.confirm_clear_at = std::time::Instant::now();
                }
            }
            // Reset the confirm flag after a timeout so a stray click does not
            // linger as a dangerous "armed" state.
            if self.confirm_clear && self.confirm_clear_at.elapsed().as_secs() > 3 {
                self.confirm_clear = false;
            }

            if ui.button("Export")
                .on_hover_text("Save the graph (items + edges) as a JSON file to your Downloads folder or configured export path.") 
                .clicked() {
                self.export_graph(None);
            }
            
            // Show export / op feedback (fades after 3 seconds). Op feedback is
            // checked first so Auto Connect / Clear messages surface promptly.
            let mut feedback_text: Option<(egui::Color32, String)> = None;
            if let Some((time, ref msg, is_success)) = self.op_feedback {
                let elapsed = time.elapsed().as_secs_f32();
                if elapsed < 3.0 {
                    let alpha = ((3.0 - elapsed) / 3.0 * 255.0) as u8;
                    let color = if is_success {
                        Color32::from_rgba_unmultiplied(0, 200, 230, alpha)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 100, 100, alpha)
                    };
                    feedback_text = Some((color, msg.clone()));
                } else {
                    self.op_feedback = None;
                }
            }
            if feedback_text.is_none() {
                if let Some((time, ref msg, is_success)) = self.export_feedback {
                    let elapsed = time.elapsed().as_secs_f32();
                    if elapsed < 3.0 {
                        let alpha = ((3.0 - elapsed) / 3.0 * 255.0) as u8;
                        let color = if is_success {
                            Color32::from_rgba_unmultiplied(100, 255, 100, alpha)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 100, 100, alpha)
                        };
                        feedback_text = Some((color, msg.clone()));
                    } else {
                        self.export_feedback = None;
                    }
                }
            }
            if let Some((color, msg)) = feedback_text {
                ui.colored_label(color, msg);
            }

            // Confirm-clear hint
            if self.confirm_clear {
                ui.colored_label(
                    Color32::from_rgb(255, 200, 0),
                    "Click Clear again to confirm",
                );
            }
            
            if self.edge_creation_source.is_some() {
                ui.colored_label(Color32::YELLOW, "Click target node (ESC to cancel)");
            }
        });

        // NOTE: the auto_connect_pending flag is cleared in set_data() when the
        // refreshed snapshot arrives — NOT here. Clearing it as soon as the
        // mutation queue drained would re-enable the button within a frame,
        // long before the LLM responds, so the user could not tell the request
        // was still running.
        // Safety net: if no snapshot returns within 90s (LLM hung or request
        // dropped), give up so the button does not stay disabled forever.
        if self.auto_connect_pending {
            if let Some(started) = self.auto_connect_started_at {
                if started.elapsed().as_secs() > 90 {
                    self.auto_connect_pending = false;
                    self.auto_connect_started_at = None;
                    self.op_feedback = Some((std::time::Instant::now(), "Auto Connect timed out".to_string(), false));
                }
            }
        }

        ui.separator();

        // Legend / status bar — single row that doubles as the legend and the
        // per-tier item counts, so the toolbar stays compact (was two crowded
        // rows). Each tier chip shows "icon label: count", and the "Load more"
        // action is a labelled control with a tooltip instead of a bare "+10".
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_4;

            // Tier legend + counts (colour dot doubles as the legend marker).
            let short_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::ShortTerm).count();
            let mid_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::MidTerm).count();
            let long_count = self.cached_items.iter().filter(|i| i.memory_type == MemoryType::LongTerm).count();

            ui.label(egui::RichText::new(format!("● Short: {}", short_count)).color(Color32::from_rgb(0, 200, 255)).size(TEXT_SM));
            if ui.small_button("Load +10").on_hover_text("Show up to 10 more Short-term items in the graph.").clicked() {
                self.short_term_limit += 10;
            }
            ui.separator();

            ui.label(egui::RichText::new(format!("● Mid: {}", mid_count)).color(Color32::from_rgb(255, 200, 0)).size(TEXT_SM));
            if ui.small_button("Load +10").on_hover_text("Show up to 10 more Mid-term items in the graph.").clicked() {
                self.mid_term_limit += 10;
            }
            ui.separator();

            ui.label(egui::RichText::new(format!("● Long: {}", long_count)).color(Color32::from_rgb(0, 255, 100)).size(TEXT_SM));
            if ui.small_button("Load +10").on_hover_text("Show up to 10 more Long-term items in the graph.").clicked() {
                self.long_term_limit += 10;
            }
            ui.separator();

            // Edge relation legend (collapsed hint + tooltip listing the keys).
            ui.menu_button(egui::RichText::new("Edges").size(TEXT_SM), |ui| {
                ui.set_min_width(160.0);
                ui.colored_label(Color32::from_rgb(255, 100, 255), "→ Translated");
                ui.colored_label(Color32::from_rgb(100, 200, 255), "→ Explained");
                ui.colored_label(Color32::from_rgb(255, 150, 100), "→ Formatted");
                ui.colored_label(Color32::from_rgb(0, 255, 100), "→ Promoted");
            });

            // "Lost" (ghost) indicator — renamed from the opaque "Ghost" with a
            // tooltip explaining what these nodes are (edge sources that no
            // longer exist, lost on reboot). The dashed grey circles are
            // rendered for orphaned edge endpoints only.
            let ghost_lbl = ui.colored_label(Color32::from_rgb(120, 120, 130), "◌ Lost");
            ghost_lbl.on_hover_text(
                "\"Lost\" nodes are edge sources that no longer exist in memory — the connected item was deleted or lost on reboot. Drawn as a dashed grey circle. They are kept only so the edges remain visible; deleting the edge removes them."
            );
        });

        ui.separator();

        // Main graph area
        let available_size = ui.available_size();
        let (response, painter) = ui.allocate_painter(available_size, Sense::click_and_drag());
        let rect = response.rect;

        // Handle panning - but NOT during marquee selection or node dragging
        if response.dragged() && self.dragging_node.is_none() && self.marquee_start.is_none() {
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

                // Draw a full edge line (no arrowheads). Guard against near-zero length deltas.
                let delta = to_screen - from_screen;
                let len_sq = delta.length_sq();
                if len_sq < 1.0 {
                    // Overlapping or extremely close nodes — skip drawing this edge to avoid numerical issues
                    continue;
                }
                let dir = delta / len_sq.sqrt();

                let node_radius = NODE_RADIUS * self.zoom;
                let start = from_screen + dir * node_radius;
                let end = to_screen - dir * node_radius;

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

                let is_selected = self.selected_nodes.contains(&item.id);
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

                // Shadow — two-layer soft shadow (deeper, more "elevated" than the
                // old single 3px/alpha-80 offset). Layer 1 is a soft outer wash;
                // layer 2 is a tighter, darker core directly under the node.
                painter.circle_filled(
                    screen_pos + Vec2::new(2.0, 4.0),
                    node_radius + 2.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 60),
                );
                painter.circle_filled(
                    screen_pos + Vec2::new(1.0, 2.0),
                    node_radius,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 110),
                );
                
                // Soft accent glow ring behind the node (subtle cyan aura). This
                // is what gives the nodes "energy" without a hard neon outline.
                // Skipped when zoomed out to avoid clutter at small scales.
                if self.zoom > 0.6 {
                    painter.circle_filled(
                        screen_pos,
                        node_radius + 4.0,
                        Color32::from_rgba_unmultiplied(0, 200, 230, 22),
                    );
                }
                
                painter.circle_filled(screen_pos, node_radius, fill_color);
                // Node outline — slightly thicker at higher zoom for crispness.
                let outline_w = (2.0 * self.zoom).clamp(1.5, 3.0);
                painter.circle_stroke(screen_pos, node_radius, Stroke::new(outline_w, Color32::WHITE));

                // Label: prefer full title; otherwise show first 3 words of content
                let raw_label = if let Some(t) = item.title.as_deref().filter(|s| !s.trim().is_empty()) {
                    t.to_string()
                } else {
                    let words: Vec<&str> = item.content.split_whitespace().collect();
                    if words.len() <= 3 {
                        words.join(" ")
                    } else {
                        words[..3].join(" ")
                    }
                };
                // Ensure labels are bounded in characters (handles CJK where whitespace splitting fails)
                let label = Self::truncate_label(&raw_label, 12);
                painter.text(
                    screen_pos,
                    egui::Align2::CENTER_CENTER,
                    &label,
                    FontId::proportional(TEXT_XS * self.zoom),
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
                FontId::proportional(TEXT_SM * self.zoom),
                ghost_color,
            );
        }

        // Draw marquee selection rectangle
        if let (Some(start), Some(current)) = (self.marquee_start, self.marquee_current) {
            let screen_start = self.world_to_screen(start, rect);
            let screen_current = self.world_to_screen(current, rect);
            let marquee_rect = Rect::from_two_pos(screen_start, screen_current);
            
            // Semi-transparent fill
            painter.rect_filled(
                marquee_rect,
                0.0,
                Color32::from_rgba_unmultiplied(100, 150, 255, 40),
            );
            // Border
            painter.rect_stroke(
                marquee_rect,
                0.0,
                Stroke::new(1.5, Color32::from_rgb(100, 150, 255)),
            );
        }

        // Match interactions
        if response.clicked() {
            if let Some(hovered) = self.hovered_node {
                if let Some(source) = self.edge_creation_source {
                    if source != hovered {
                        self.mutations.push(GraphRequest::AddUserEdge { source, target: hovered });
                    }
                    self.edge_creation_source = None;
                } else {
                    // Multi-select with Ctrl+Click
                    let ctrl_held = ui.input(|i| i.modifiers.ctrl);
                    if ctrl_held {
                        // Toggle selection
                        if self.selected_nodes.contains(&hovered) {
                            self.selected_nodes.remove(&hovered);
                        } else {
                            self.selected_nodes.insert(hovered);
                        }
                    } else {
                        // Single click: clear and select one
                        self.selected_nodes.clear();
                        self.selected_nodes.insert(hovered);
                    }
                }
            } else {
                self.selected_nodes.clear();
                self.edge_creation_source = None;
            }
        }

        // Right-click context menu for delete
        if response.secondary_clicked() {
            if let Some(hovered) = self.hovered_node {
                // Ensure right-clicked node is selected
                if !self.selected_nodes.contains(&hovered) {
                    self.selected_nodes.clear();
                    self.selected_nodes.insert(hovered);
                }
            }
        }
        
        // Context menu popup
        response.context_menu(|ui| {
            if !self.selected_nodes.is_empty() {
                let count = self.selected_nodes.len();
                let label = if count == 1 {
                    "🗑 Delete item".to_string()
                } else {
                    format!("🗑 Delete {} items", count)
                };
                if ui.button(label).clicked() {
                    for id in self.selected_nodes.drain() {
                        self.mutations.push(GraphRequest::DeleteItem { id });
                    }
                    self.title_edit_id = None;
                    self.title_edit_buffer.clear();
                    self.title_edit_dirty = false;
                    ui.close_menu();
                }
            }
        });

        if response.drag_started() {
            let shift_held = ui.input(|i| i.modifiers.shift);
            if shift_held && self.hovered_node.is_none() {
                // Shift+Drag on empty area: start marquee selection
                if let Some(pointer) = pointer_pos {
                    self.marquee_start = Some(self.screen_to_world(pointer, rect));
                    self.marquee_current = self.marquee_start;
                }
            } else if let Some(hovered) = self.hovered_node {
                // Drag on a node: move it
                self.dragging_node = Some(hovered);
                if let Some(&pos) = self.node_positions.get(&hovered) {
                    let screen_pos = self.world_to_screen(pos, rect);
                    self.drag_offset = pointer_pos.unwrap_or(screen_pos) - screen_pos;
                }
            }
        }

        if response.dragged() {
            if self.marquee_start.is_some() {
                // Update marquee rectangle
                if let Some(pointer) = pointer_pos {
                    self.marquee_current = Some(self.screen_to_world(pointer, rect));
                }
            } else if let Some(dragging) = self.dragging_node {
                if let Some(pointer) = pointer_pos {
                    let new_screen_pos = pointer - self.drag_offset;
                    let new_world_pos = self.screen_to_world(new_screen_pos, rect);
                    self.node_positions.insert(dragging, new_world_pos);
                }
            }
        }

        if response.drag_stopped() {
            if let (Some(start), Some(end)) = (self.marquee_start.take(), self.marquee_current.take()) {
                // Finalize marquee selection
                let min_x = start.x.min(end.x);
                let max_x = start.x.max(end.x);
                let min_y = start.y.min(end.y);
                let max_y = start.y.max(end.y);
                let selection_rect = Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y));
                
                let ctrl_held = ui.input(|i| i.modifiers.ctrl);
                if !ctrl_held {
                    self.selected_nodes.clear();
                }
                
                // Find all nodes inside the rectangle
                for item in &self.cached_items {
                    if let Some(&pos) = self.node_positions.get(&item.id) {
                        if selection_rect.contains(pos) {
                            self.selected_nodes.insert(item.id);
                        }
                    }
                }
            } else if let Some(dragging) = self.dragging_node.take() {
                // Persist node position only when drag ends.
                if let Some(pos) = self.node_positions.get(&dragging).copied() {
                    // Instead of direct DB update, send mutation request
                    self.mutations.push(GraphRequest::UpdateNodePosition { id: dragging, x: pos.x, y: pos.y });
                }
            }
        }

        // ESC to cancel edge creation / clear selection
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.edge_creation_source = None;
            self.selected_nodes.clear();
            self.title_edit_id = None;
            self.title_edit_buffer.clear();
            self.title_edit_dirty = false;
        }

        // Delete key to delete selected nodes
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !self.selected_nodes.is_empty() {
            for id in self.selected_nodes.drain() {
                self.mutations.push(GraphRequest::DeleteItem { id });
            }
            self.title_edit_id = None;
            self.title_edit_buffer.clear();
            self.title_edit_dirty = false;
        }

        // Selected node details panel (only when exactly one node is selected)
        if self.selected_nodes.len() == 1 {
            let selected_id = *self.selected_nodes.iter().next().unwrap();
            // Find item in cache instead of store.get_item
            if let Some(item) = self.cached_items.iter().find(|i| i.id == selected_id).cloned() {
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
                            ActionType::Format => "Format",
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
                // Editable title - only reset buffer when switching to a different item AND not dirty
                if self.title_edit_id != Some(item.id) {
                    // Switching to a new item: reset buffer
                    self.title_edit_id = Some(item.id);
                    self.title_edit_buffer = item.title.clone().unwrap_or_default();
                    self.title_edit_dirty = false;
                } else if self.title_edit_dirty {
                    // Dirty: check if server confirmed our update (titles match)
                    let server_title = item.title.clone().unwrap_or_default();
                    if server_title == self.title_edit_buffer {
                        self.title_edit_dirty = false; // Server confirmed
                    }
                    // Otherwise keep our local buffer
                } else {
                    // Not dirty: sync with server data
                    self.title_edit_buffer = item.title.clone().unwrap_or_default();
                }

                ui.horizontal(|ui| {
                    ui.label("Title:");
                    let title_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.title_edit_buffer)
                            .desired_width(200.0)
                            .hint_text("Optional title"),
                    );

                    // Track if user modified the buffer
                    if title_resp.changed() {
                        self.title_edit_dirty = true;
                    }

                    // Save button
                    if ui.button("💾").on_hover_text("Save title").clicked() {
                        eprintln!("Save button clicked, pushing UpdateItemTitle for {}", item.id);
                        self.mutations.push(GraphRequest::UpdateItemTitle { 
                            id: item.id, 
                            title: self.title_edit_buffer.clone() 
                        });
                    }

                    // Title is saved when Enter is pressed while focused, then focus is surrendered.
                    if title_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        eprintln!("Enter pressed, pushing UpdateItemTitle for {}", item.id);
                        self.mutations.push(GraphRequest::UpdateItemTitle { 
                            id: item.id, 
                            title: self.title_edit_buffer.clone() 
                        });
                        ui.ctx().memory_mut(|mem| mem.surrender_focus(title_resp.id));
                    }
                });

                ui.label("Content:");
                // Avoid cloning large content if possible, but egui needs mutable ref or we copy.
                // Cloning string is fine for UI loop for now.
                let mut content = item.content.clone();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut content)
                            .desired_width(f32::INFINITY)
                            .interactive(false));
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if item.memory_type != MemoryType::MidTerm {
                        if ui.button("→ Mid").clicked() {
                            self.mutations.push(GraphRequest::PromoteItem {
                                id: item.id,
                                target_type: MemoryType::MidTerm
                            });
                             // Selection update should happen after refresh, but we can't easily predict the new ID.
                             // Client will receive new snapshot.
                        }
                    }
                    if item.memory_type != MemoryType::LongTerm {
                        if ui.button("→ Long").clicked() {
                             self.mutations.push(GraphRequest::PromoteItem {
                                id: item.id,
                                target_type: MemoryType::LongTerm
                            });
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
                        self.mutations.push(GraphRequest::DeleteItem { id: item.id });
                        self.selected_nodes.clear();
                        self.title_edit_id = None;
                        self.title_edit_buffer.clear();
                        self.title_edit_dirty = false;
                    }
                });
            });
    }

    pub fn export_graph(&mut self, path: Option<std::path::PathBuf>) {
        use chrono::Utc;
        use std::fs::File;
        use std::io::Write;

        // Serialize synchronously for immediate feedback
        let payload = serde_json::json!({ "items": self.cached_items, "edges": self.cached_edges });
        let data = match serde_json::to_string_pretty(&payload) {
            Ok(s) => s,
            Err(e) => {
                self.export_feedback = Some((std::time::Instant::now(), format!("Export failed: {}", e), false));
                return;
            }
        };

        let out_path = match path {
            Some(p) => p,
            None => {
                // Use configured export_path, fall back to Downloads
                let mut dir = if let Some(ref configured) = self.export_path {
                    std::path::PathBuf::from(configured)
                } else {
                    dirs::download_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                };
                let ts = Utc::now().format("%Y%m%d_%H%M%S");
                dir.push(format!("memory_graph_{}.json", ts));
                dir
            }
        };

        match File::create(&out_path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(data.as_bytes()) {
                    self.export_feedback = Some((std::time::Instant::now(), format!("Write failed: {}", e), false));
                } else {
                    let filename = out_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file");
                    self.export_feedback = Some((std::time::Instant::now(), format!("✓ Saved: {}", filename), true));
                }
            }
            Err(e) => {
                self.export_feedback = Some((std::time::Instant::now(), format!("Create failed: {}", e), false));
            }
        }
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
