//! Feature Panel - A tree view for displaying Manifest features.
//!
//! Uses gpui-component's Tree for keyboard navigation and virtualized rendering.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, Render, SharedString,
    Styled, Window, actions, div, px, rgba,
};

// Action for opening the selected feature
actions!(feature_panel, [OpenFeature]);
use gpui_component::list::ListItem;
use gpui_component::tree::{TreeItem, TreeState, tree};
use gpui_component::{Icon, IconName};
use manifest_client::{Feature, FeatureState};
use uuid::Uuid;

/// Colors for the feature panel (Pigs in Space theme).
mod colors {
    use gpui::Rgba;

    pub fn header_background() -> Rgba {
        Rgba {
            r: 0.082,
            g: 0.098,
            b: 0.118,
            a: 1.0,
        } // #15191e - darker for title bar
    }

    pub fn panel_background() -> Rgba {
        Rgba {
            r: 0.114,
            g: 0.133,
            b: 0.157,
            a: 1.0,
        } // #1d2228 - panel.background
    }

    pub fn text_primary() -> Rgba {
        Rgba {
            r: 0.761,
            g: 0.839,
            b: 0.918,
            a: 1.0,
        } // #c2d6ea - text
    }

    pub fn text_muted() -> Rgba {
        Rgba {
            r: 0.471,
            g: 0.522,
            b: 0.608,
            a: 1.0,
        } // #78859b - text.muted
    }

    // State icon colors (Pigs in Space theme-aligned)
    pub fn proposed_amber() -> Rgba {
        Rgba {
            r: 0.973,
            g: 0.745,
            b: 0.325,
            a: 1.0,
        } // #f8be53 - warning
    }

    pub fn in_progress_green() -> Rgba {
        Rgba {
            r: 0.765,
            g: 0.910,
            b: 0.553,
            a: 1.0,
        } // #c3e88d - success/green
    }

    pub fn implemented_blue() -> Rgba {
        Rgba {
            r: 0.510,
            g: 0.667,
            b: 1.0,
            a: 1.0,
        } // #82aaff - info/blue
    }

    pub fn implemented_check() -> Rgba {
        Rgba {
            r: 0.129,
            g: 0.149,
            b: 0.173,
            a: 1.0,
        } // #21262c - dark checkmark
    }

    pub fn deprecated_gray() -> Rgba {
        Rgba {
            r: 0.388,
            g: 0.431,
            b: 0.502,
            a: 1.0,
        } // #636e80 - text.muted
    }
}

/// Events emitted by the FeaturePanel.
#[derive(Clone, Debug)]
pub enum Event {
    FeatureSelected(Uuid),
}

/// State of data loading.
#[derive(Clone, Debug)]
pub enum LoadState {
    Loading,
    Loaded,
    Error(String),
}

/// Default panel width.
pub const DEFAULT_PANEL_WIDTH: f32 = 250.0;
/// Minimum panel width.
pub const MIN_PANEL_WIDTH: f32 = 150.0;
/// Maximum panel width.
pub const MAX_PANEL_WIDTH: f32 = 500.0;

/// Metadata for a feature (state, has_children) keyed by tree item ID.
#[derive(Clone)]
struct FeatureMetadata {
    id: Uuid,
    state: FeatureState,
    has_children: bool,
}

/// Special ID used for the directory root node.
const DIRECTORY_ROOT_ID: &str = "__directory_root__";

/// GPUI Entity for displaying a feature tree.
pub struct FeaturePanel {
    tree_state: Entity<TreeState>,
    focus_handle: FocusHandle,
    load_state: LoadState,
    width: f32,
    /// Metadata for features, keyed by tree item ID string.
    /// Wrapped in Rc for cheap cloning in render closures.
    feature_metadata: Rc<HashMap<String, FeatureMetadata>>,
    /// Flag to track if selection change was caused by a click.
    pending_click_open: bool,
    /// Directory name shown as root node of the feature tree.
    directory_name: Option<String>,
}

impl FeaturePanel {
    /// Create a new empty feature panel.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));

        // Observe tree state changes to detect click-triggered selection
        cx.observe(&tree_state, |this, _state, cx| {
            this.on_tree_selection_changed(cx);
        })
        .detach();

        Self {
            tree_state,
            focus_handle: cx.focus_handle(),
            load_state: LoadState::Loading,
            width: DEFAULT_PANEL_WIDTH,
            feature_metadata: Rc::new(HashMap::new()),
            pending_click_open: false,
            directory_name: None,
        }
    }

    /// Set the panel width.
    pub fn set_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.width = width.clamp(MIN_PANEL_WIDTH, MAX_PANEL_WIDTH);
        cx.notify();
    }

    /// Get the current panel width.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Set the features to display, with optional directory name as root.
    pub fn set_features(
        &mut self,
        features: Vec<Feature>,
        directory_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.directory_name = directory_name.clone();

        // Convert features to TreeItems and collect metadata
        let (tree_items, metadata) =
            Self::convert_features_to_tree_items(&features, directory_name.as_deref());
        self.feature_metadata = Rc::new(metadata);

        // Update tree state
        self.tree_state.update(cx, |state, cx| {
            state.set_items(tree_items, cx);
        });

        self.load_state = LoadState::Loaded;
        cx.notify();
    }

    /// Set an error state.
    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.load_state = LoadState::Error(error);
        cx.notify();
    }

    /// Handle tree selection changes - only open if triggered by a click.
    fn on_tree_selection_changed(&mut self, cx: &mut Context<Self>) {
        if self.pending_click_open {
            self.pending_click_open = false;
            self.open_selected_feature(cx);
        }
    }

    /// Open the currently selected feature (emit FeatureSelected event).
    /// Called when user presses Enter or clicks on a leaf item.
    fn open_selected_feature(&mut self, cx: &mut Context<Self>) {
        let selected_id = self
            .tree_state
            .read(cx)
            .selected_item()
            .map(|item| item.id.to_string());

        if let Some(id) = selected_id {
            if let Some(metadata) = self.feature_metadata.get(&id) {
                // Only open leaf features (non-folders)
                if !metadata.has_children {
                    cx.emit(Event::FeatureSelected(metadata.id));
                }
            }
        }
    }

    /// Action handler for OpenFeature (Enter key).
    fn on_open_feature(&mut self, _: &OpenFeature, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_selected_feature(cx);
    }

    /// Convert Feature tree to TreeItem tree, collecting metadata.
    /// If directory_name is provided, wraps all features under a root directory node.
    fn convert_features_to_tree_items(
        features: &[Feature],
        directory_name: Option<&str>,
    ) -> (Vec<TreeItem>, HashMap<String, FeatureMetadata>) {
        let mut metadata = HashMap::new();
        let feature_items = Self::convert_features_recursive(features, &mut metadata);

        // If we have a directory name, wrap features under a root node
        let items = if let Some(dir_name) = directory_name {
            vec![
                TreeItem::new(DIRECTORY_ROOT_ID, dir_name.to_string())
                    .children(feature_items)
                    .expanded(true),
            ] // Directory starts expanded
        } else {
            feature_items
        };

        (items, metadata)
    }

    fn convert_features_recursive(
        features: &[Feature],
        metadata: &mut HashMap<String, FeatureMetadata>,
    ) -> Vec<TreeItem> {
        features
            .iter()
            .map(|feature| {
                let id_str = feature.id.to_string();
                let has_children = !feature.children.is_empty();

                // Store metadata keyed by item ID
                metadata.insert(
                    id_str.clone(),
                    FeatureMetadata {
                        id: feature.id,
                        state: feature.state,
                        has_children,
                    },
                );

                let children = Self::convert_features_recursive(&feature.children, metadata);

                TreeItem::new(id_str, feature.title.clone())
                    .children(children)
                    .expanded(false) // Start collapsed
            })
            .collect()
    }

    /// Render a folder icon using gpui-component's SVG icons.
    fn render_folder_icon(is_expanded: bool) -> impl IntoElement {
        let icon_name = if is_expanded {
            IconName::FolderOpen
        } else {
            IconName::FolderClosed
        };
        Icon::new(icon_name)
            .size_4()
            .text_color(Hsla::from(colors::text_muted()))
    }

    /// Render a proposed state icon: small amber solid circle.
    /// Using a simple single div for performance.
    fn render_proposed_icon() -> impl IntoElement {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded(px(4.0))
                    .bg(colors::proposed_amber()),
            )
    }

    /// Render an in_progress state icon: green donut/ring.
    /// Using a simple single div with border for performance.
    fn render_in_progress_icon() -> impl IntoElement {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(12.0))
                    .h(px(12.0))
                    .rounded(px(6.0))
                    .border_2()
                    .border_color(colors::in_progress_green()),
            )
    }

    /// Render an implemented state icon: solid light blue circle.
    /// Same radius as in_progress icon (12x12).
    fn render_implemented_icon() -> impl IntoElement {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(12.0))
                    .h(px(12.0))
                    .rounded(px(6.0))
                    .bg(colors::implemented_blue()),
            )
    }

    /// Render a deprecated state icon using gpui-component's Inbox (archive-like).
    fn render_deprecated_icon() -> impl IntoElement {
        Icon::new(IconName::Inbox)
            .size_4()
            .text_color(Hsla::from(colors::deprecated_gray()))
    }
}

impl EventEmitter<Event> for FeaturePanel {}

impl Focusable for FeaturePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FeaturePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel_bg = colors::panel_background();

        // Cheap Rc clone for use in render closure (just increments refcount)
        let metadata = Rc::clone(&self.feature_metadata);

        div()
            .id("feature-panel")
            .size_full()
            .bg(panel_bg)
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("FeaturePanel")
            .on_action(cx.listener(Self::on_open_feature))
            .child(
                // Header
                div()
                    .h(px(32.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .bg(colors::header_background())
                    .border_b_1()
                    .border_color(rgba(0x2d333aff))
                    .child(
                        div()
                            .font_family("IBM Plex Sans")
                            .text_color(colors::text_primary())
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("MANIFEST"),
                    ),
            )
            .child(
                // Tree content
                div()
                    .id("feature-list-container")
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    // Set flag on mouse down to trigger open on selection change
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _cx| {
                            this.pending_click_open = true;
                        }),
                    )
                    .child(match &self.load_state {
                        LoadState::Loading => div()
                            .p(px(12.0))
                            .font_family("IBM Plex Sans")
                            .text_color(colors::text_muted())
                            .text_size(px(13.0))
                            .child("Loading features...")
                            .into_any_element(),
                        LoadState::Error(err) => div()
                            .p(px(12.0))
                            .font_family("IBM Plex Sans")
                            .text_color(rgba(0xf14c4cff))
                            .text_size(px(13.0))
                            .child(format!("Error: {}", err))
                            .into_any_element(),
                        LoadState::Loaded => {
                            tree(
                                &self.tree_state,
                                move |_ix, entry, selected, _window, _cx| {
                                    let depth = entry.depth();
                                    let indent = px(16.0 * depth as f32 + 8.0);
                                    let is_expanded = entry.is_expanded();
                                    let is_folder = entry.is_folder();
                                    let item_id = entry.item().id.to_string();
                                    let label: SharedString = entry.item().label.clone();

                                    // Get metadata for this entry to render custom icon
                                    let meta = metadata.get(&item_id);

                                    // Render custom icon based on feature state
                                    let icon = if is_folder {
                                        Self::render_folder_icon(is_expanded).into_any_element()
                                    } else if let Some(m) = meta {
                                        match m.state {
                                            FeatureState::Proposed => {
                                                Self::render_proposed_icon().into_any_element()
                                            }
                                            FeatureState::InProgress => {
                                                Self::render_in_progress_icon().into_any_element()
                                            }
                                            FeatureState::Implemented => {
                                                Self::render_implemented_icon().into_any_element()
                                            }
                                            FeatureState::Deprecated => {
                                                Self::render_deprecated_icon().into_any_element()
                                            }
                                        }
                                    } else {
                                        Self::render_proposed_icon().into_any_element()
                                    };

                                    ListItem::new(item_id)
                                        .py_0()
                                        .pl(indent)
                                        .selected(selected)
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .child(icon)
                                                .child(
                                                    div()
                                                        .font_family("IBM Plex Sans")
                                                        .text_color(colors::text_primary())
                                                        .text_size(px(13.0))
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .child(label),
                                                ),
                                        )
                                },
                            )
                            .size_full()
                            .into_any_element()
                        }
                    }),
            )
    }
}

/// Register key bindings for the feature panel.
pub fn register_bindings(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("enter", OpenFeature, Some("FeaturePanel"))]);
}
