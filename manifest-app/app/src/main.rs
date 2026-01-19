//! Manifest Terminal Application
//!
//! A GPUI application with a feature explorer panel, feature editor, and terminal.

mod active_context;
mod config;
mod context_file;

use active_context::ActiveFeatureContext;
use config::AppConfig;

use feature_editor::{Event as EditorEvent, FeatureEditor};
use feature_panel::{
    DEFAULT_PANEL_WIDTH, Event as PanelEvent, FeaturePanel, MAX_PANEL_WIDTH, MIN_PANEL_WIDTH,
};
use gpui::{
    App, Application, Bounds, Context, Entity, Focusable, Hsla, KeyBinding, Menu, MenuItem,
    ParentElement, PathPromptOptions, Render, Styled, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, point, prelude::*, px, size,
};
use gpui_component::Root;
use gpui_component::highlighter::{HighlightTheme, HighlightThemeStyle};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::theme::{Theme, ThemeMode};
use manifest_core::db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use terminal::mappings::colors::TerminalColors;
use terminal_view::TerminalView;
use uuid::Uuid;

/// Convert manifest_core types to manifest_client types for feature_panel compatibility.
mod convert {
    use manifest_client::{Feature, FeatureState};
    use manifest_core::models::{FeatureState as CoreState, FeatureTreeNode};

    fn convert_state(state: CoreState) -> FeatureState {
        match state {
            CoreState::Proposed => FeatureState::Proposed,
            CoreState::InProgress => FeatureState::InProgress,
            CoreState::Implemented => FeatureState::Implemented,
            CoreState::Deprecated => FeatureState::Deprecated,
        }
    }

    pub fn tree_node_to_feature(node: FeatureTreeNode) -> Feature {
        Feature {
            id: node.feature.id,
            project_id: node.feature.project_id,
            parent_id: node.feature.parent_id,
            title: node.feature.title,
            details: node.feature.details,
            desired_details: node.feature.desired_details,
            state: convert_state(node.feature.state),
            priority: node.feature.priority,
            created_at: node.feature.created_at.to_rfc3339(),
            updated_at: node.feature.updated_at.to_rfc3339(),
            children: node
                .children
                .into_iter()
                .map(tree_node_to_feature)
                .collect(),
        }
    }
}

actions!(app, [Quit, Open, OpenRecent, Save]);

/// Load embedded fonts into the text system.
fn load_embedded_fonts(cx: &App) {
    use std::borrow::Cow;

    let fonts: Vec<Cow<'static, [u8]>> = vec![
        // IBM Plex Sans (UI font)
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexSans-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf")),
        // IBM Plex Mono (editor font)
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexMono-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/IBMPlexMono-SemiBold.ttf")),
        // Bitstream Vera Sans Mono (terminal font)
        Cow::Borrowed(include_bytes!("../fonts/VeraMono.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/VeraMoBd.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/VeraMoIt.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/VeraMoBI.ttf")),
    ];

    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("Failed to load embedded fonts: {}", e);
    }
}

/// Apply "Pigs in Space" theme colors to gpui-component.
fn apply_pigs_in_space_theme(cx: &mut App) {
    let theme = Theme::global_mut(cx);

    // Editor font
    theme.mono_font_family = "Bitstream Vera Sans Mono".into();

    // Background colors
    theme.background = Hsla {
        h: 210.0 / 360.0,
        s: 0.13,
        l: 0.15,
        a: 1.0,
    }; // #21262c

    // Text colors
    theme.foreground = Hsla {
        h: 210.0 / 360.0,
        s: 0.45,
        l: 0.84,
        a: 1.0,
    }; // #c2d6ea
    theme.muted_foreground = Hsla {
        h: 215.0 / 360.0,
        s: 0.20,
        l: 0.55,
        a: 1.0,
    }; // #78859b

    // Cursor/caret color (bright for visibility on dark background)
    theme.caret = Hsla {
        h: 210.0 / 360.0,
        s: 0.45,
        l: 0.84,
        a: 1.0,
    }; // same as foreground

    // Border color (for dividers)
    theme.border = Hsla {
        h: 210.0 / 360.0,
        s: 0.10,
        l: 0.28,
        a: 1.0,
    }; // ~#3a4149

    // Tab bar colors
    theme.tab_bar = Hsla {
        h: 212.0 / 360.0,
        s: 0.15,
        l: 0.10,
        a: 1.0,
    }; // #15191e
    theme.tab = Hsla {
        h: 212.0 / 360.0,
        s: 0.15,
        l: 0.10,
        a: 1.0,
    }; // same as tab bar
    theme.tab_active = Hsla {
        h: 210.0 / 360.0,
        s: 0.13,
        l: 0.15,
        a: 1.0,
    }; // #21262c
    theme.tab_foreground = Hsla {
        h: 215.0 / 360.0,
        s: 0.20,
        l: 0.55,
        a: 1.0,
    }; // muted
    theme.tab_active_foreground = Hsla {
        h: 210.0 / 360.0,
        s: 0.45,
        l: 0.84,
        a: 1.0,
    }; // bright

    // List/hover colors (used for tab hover, tree items, etc.)
    // From Pigs in Space: list.hoverBackground = #30353d, list.activeSelectionBackground = #373f47
    theme.list_hover = Hsla {
        h: 215.0 / 360.0,
        s: 0.12,
        l: 0.21,
        a: 1.0,
    }; // #30353d
    theme.list_active = Hsla {
        h: 210.0 / 360.0,
        s: 0.12,
        l: 0.25,
        a: 1.0,
    }; // #373f47
    theme.list_active_border = Hsla {
        h: 210.0 / 360.0,
        s: 0.10,
        l: 0.20,
        a: 0.0,
    }; // transparent

    // Secondary colors (used for ghost buttons)
    theme.secondary = Hsla {
        h: 212.0 / 360.0,
        s: 0.15,
        l: 0.15,
        a: 1.0,
    };
    theme.secondary_hover = Hsla {
        h: 212.0 / 360.0,
        s: 0.12,
        l: 0.22,
        a: 1.0,
    };
    theme.secondary_foreground = Hsla {
        h: 215.0 / 360.0,
        s: 0.20,
        l: 0.55,
        a: 1.0,
    };

    // Accent colors
    theme.accent = Hsla {
        h: 212.0 / 360.0,
        s: 0.12,
        l: 0.22,
        a: 1.0,
    };
    theme.accent_foreground = Hsla {
        h: 210.0 / 360.0,
        s: 0.45,
        l: 0.84,
        a: 1.0,
    };

    // Editor/highlight theme colors (for Input component with line numbers)
    // From Pigs in Space Zed theme:
    // editor.background: #21262c, editor.foreground: #a0b0c1
    // editor.active_line.background: #2c3137
    // editor.line_number: #424b55, editor.active_line_number: #a0b0c1
    theme.highlight_theme = Arc::new(HighlightTheme {
        name: "Pigs in Space".into(),
        appearance: ThemeMode::Dark,
        style: HighlightThemeStyle {
            editor_background: Some(Hsla {
                h: 210.0 / 360.0,
                s: 0.13,
                l: 0.15,
                a: 1.0,
            }), // #21262c
            editor_foreground: Some(Hsla {
                h: 210.0 / 360.0,
                s: 0.18,
                l: 0.66,
                a: 1.0,
            }), // #a0b0c1
            editor_active_line: Some(Hsla {
                h: 212.0 / 360.0,
                s: 0.10,
                l: 0.19,
                a: 1.0,
            }), // #2c3137
            editor_line_number: Some(Hsla {
                h: 212.0 / 360.0,
                s: 0.12,
                l: 0.30,
                a: 1.0,
            }), // #424b55
            editor_active_line_number: Some(Hsla {
                h: 210.0 / 360.0,
                s: 0.18,
                l: 0.66,
                a: 1.0,
            }), // #a0b0c1
            ..Default::default()
        },
    });
}

/// Set up the application menus.
fn set_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Manifest".into(),
            items: vec![MenuItem::action("Quit Manifest", Quit)],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open Working Directory...", Open),
                MenuItem::action("Open Recent", OpenRecent),
                MenuItem::separator(),
                MenuItem::action("Save", Save),
            ],
        },
    ]);
}

/// Result of fetching features: features, the directory name to display, and the project path.
struct FetchResult {
    features: Vec<manifest_client::Feature>,
    directory_name: Option<String>,
    /// The project directory path (for writing per-project context).
    project_path: Option<PathBuf>,
}

/// Root application view with feature panel, editor, and terminal.
pub struct ManifestApp {
    feature_panel: Entity<FeaturePanel>,
    feature_editor: Entity<FeatureEditor>,
    terminal_view: Entity<TerminalView>,
    config: AppConfig,
    current_project_path: Option<PathBuf>,
}

impl ManifestApp {
    pub fn new(config: AppConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let feature_panel = cx.new(|cx| {
            let mut panel = FeaturePanel::new(cx);
            if let Some(width) = config.feature_panel_width {
                panel.set_width(width, cx);
            }
            panel
        });
        let feature_editor = cx.new(|cx| FeatureEditor::new(cx));
        let terminal_view = cx.new(|cx| TerminalView::new(window, cx));

        // Subscribe to feature panel selection events
        cx.subscribe(&feature_panel, |this, _panel, event: &PanelEvent, cx| {
            let PanelEvent::FeatureSelected(feature_id) = event;
            this.on_feature_selected(*feature_id, cx);
        })
        .detach();

        // Subscribe to editor events
        cx.subscribe(
            &feature_editor,
            |_this, _editor, event: &EditorEvent, _cx| match event {
                EditorEvent::FeatureSaved(id) => {
                    eprintln!("Feature {} saved", id);
                }
                EditorEvent::SaveFailed(id, err) => {
                    eprintln!("Failed to save feature {}: {}", id, err);
                }
            },
        )
        .detach();

        // Focus the terminal on startup
        let focus_handle = terminal_view.focus_handle(cx);
        focus_handle.focus(window, cx);

        // Fetch features in background
        let feature_panel_clone = feature_panel.clone();
        let background_executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = background_executor
                .spawn(async move { Self::fetch_features() })
                .await;

            match result {
                Ok(FetchResult {
                    features,
                    directory_name,
                    project_path,
                }) => {
                    eprintln!("Loaded {} features", features.len());
                    // Save project path for context file writing
                    if let Some(this) = this.upgrade() {
                        cx.update_entity(&this, |app, _cx| {
                            app.current_project_path = project_path;
                        });
                    }
                    cx.update_entity(&feature_panel_clone, |panel, cx| {
                        panel.set_features(features, directory_name, cx);
                    });
                }
                Err(e) => {
                    eprintln!("Failed to load features: {}", e);
                    cx.update_entity(&feature_panel_clone, |panel, cx| {
                        panel.set_error(e, cx);
                    });
                }
            }
        })
        .detach();

        Self {
            feature_panel,
            feature_editor,
            terminal_view,
            config,
            current_project_path: None,
        }
    }

    /// Handle feature selection from the panel.
    fn on_feature_selected(&self, feature_id: Uuid, cx: &mut Context<Self>) {
        let editor_clone = self.feature_editor.clone();
        let project_path = self.current_project_path.clone();
        let background_executor = cx.background_executor().clone();

        cx.spawn(async move |_this, cx| {
            let result = background_executor
                .spawn(async move {
                    let db = Database::open_default()?;
                    db.get_feature(feature_id)
                })
                .await;

            match result {
                Ok(Some(feature)) => {
                    // Write to context file for MCP server (per-project)
                    if let Some(ref path) = project_path {
                        if let Err(e) = context_file::write_context(
                            path,
                            feature.id,
                            &feature.title,
                            feature.details.as_deref(),
                        ) {
                            eprintln!("Failed to write context file: {}", e);
                        }
                    }

                    // Update global active feature context
                    cx.update(|cx| {
                        ActiveFeatureContext::set(
                            ActiveFeatureContext {
                                feature_id: Some(feature.id),
                                feature_title: Some(feature.title.clone()),
                                feature_details: feature.details.clone(),
                            },
                            cx,
                        );
                    });

                    // Update editor - use update_entity which works without window handle
                    cx.update_entity(&editor_clone, |editor, cx| {
                        editor.load_feature(feature.id, feature.title, feature.details, cx);
                    });
                }
                Ok(None) => {
                    eprintln!("Feature not found: {}", feature_id);
                }
                Err(e) => {
                    eprintln!("Failed to load feature: {}", e);
                }
            }
        })
        .detach();
    }

    /// Fetch features for a specific directory path (blocking, runs on background thread).
    fn fetch_features_for_path(path: &str) -> Result<FetchResult, String> {
        let db = Database::open_default().map_err(|e| format!("Failed to open database: {}", e))?;
        db.migrate()
            .map_err(|e| format!("Failed to migrate database: {}", e))?;

        // Try to find project by directory
        if let Ok(Some(project_with_dirs)) = db.get_project_by_directory(path) {
            eprintln!(
                "Found project '{}' for directory",
                project_with_dirs.project.name
            );
            match db.get_feature_tree(project_with_dirs.project.id) {
                Ok(features) => {
                    eprintln!(
                        "Loaded {} features from '{}'",
                        features.len(),
                        project_with_dirs.project.name
                    );
                    let converted: Vec<_> = features
                        .into_iter()
                        .map(convert::tree_node_to_feature)
                        .collect();

                    // Extract directory name from path
                    let directory_name = std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());

                    return Ok(FetchResult {
                        features: converted,
                        directory_name,
                        project_path: Some(PathBuf::from(path)),
                    });
                }
                Err(e) => {
                    eprintln!("Error fetching features: {}", e);
                }
            }
        }

        Err(format!("No project found for directory: {}", path))
    }

    /// Fetch features, trying CWD first then falling back to any project with features.
    fn fetch_features() -> Result<FetchResult, String> {
        let db = Database::open_default().map_err(|e| format!("Failed to open database: {}", e))?;
        db.migrate()
            .map_err(|e| format!("Failed to migrate database: {}", e))?;

        // Try current working directory first
        if let Ok(cwd) = std::env::current_dir() {
            if let Some(cwd_str) = cwd.to_str() {
                if let Ok(Some(project_with_dirs)) = db.get_project_by_directory(cwd_str) {
                    eprintln!(
                        "Found project '{}' for current directory",
                        project_with_dirs.project.name
                    );
                    match db.get_feature_tree(project_with_dirs.project.id) {
                        Ok(features) => {
                            eprintln!(
                                "Loaded {} features from '{}'",
                                features.len(),
                                project_with_dirs.project.name
                            );
                            let converted: Vec<_> = features
                                .into_iter()
                                .map(convert::tree_node_to_feature)
                                .collect();

                            // Extract directory name from CWD
                            let directory_name = cwd
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string());

                            return Ok(FetchResult {
                                features: converted,
                                directory_name,
                                project_path: Some(cwd),
                            });
                        }
                        Err(e) => {
                            eprintln!("Error fetching features: {}", e);
                        }
                    }
                }
            }
        }

        // Fallback: find first project with features (no directory context)
        let projects = db
            .get_all_projects()
            .map_err(|e| format!("Failed to fetch projects: {}", e))?;

        for project in &projects {
            match db.get_feature_tree(project.id) {
                Ok(features) if !features.is_empty() => {
                    eprintln!(
                        "Found {} features in project '{}'",
                        features.len(),
                        project.name
                    );
                    let converted: Vec<_> = features
                        .into_iter()
                        .map(convert::tree_node_to_feature)
                        .collect();
                    return Ok(FetchResult {
                        features: converted,
                        directory_name: None, // No directory context in fallback
                        project_path: None,   // No project path in fallback
                    });
                }
                Ok(_) => continue,
                Err(e) => {
                    eprintln!("Error fetching features for '{}': {}", project.name, e);
                }
            }
        }

        Err("No projects with features found".into())
    }

    /// Open a project from a directory path and load its features.
    fn open_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path_str = path.to_string_lossy().to_string();
        self.current_project_path = Some(path);

        let feature_panel = self.feature_panel.clone();
        let background_executor = cx.background_executor().clone();

        cx.spawn(async move |this, cx| {
            let result = background_executor
                .spawn(async move { Self::fetch_features_for_path(&path_str) })
                .await;

            match result {
                Ok(FetchResult {
                    features,
                    directory_name,
                    project_path,
                }) => {
                    eprintln!("Loaded {} features", features.len());
                    // Update project path in app state
                    if let Some(this) = this.upgrade() {
                        cx.update_entity(&this, |app, _cx| {
                            app.current_project_path = project_path;
                        });
                    }
                    cx.update_entity(&feature_panel, |panel, cx| {
                        panel.set_features(features, directory_name, cx);
                    });
                }
                Err(e) => {
                    eprintln!("Failed to load features: {}", e);
                    cx.update_entity(&feature_panel, |panel, cx| {
                        panel.set_error(e, cx);
                    });
                }
            }
        })
        .detach();
    }
}

impl Render for ManifestApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg_color: gpui::Hsla = TerminalColors::background().into();

        div()
            .id("manifest-app")
            .size_full()
            .bg(bg_color)
            .child(
                // Horizontal split: feature panel | editor+terminal
                h_resizable("main-layout")
                    .child(
                        resizable_panel()
                            .size(px(self
                                .config
                                .feature_panel_width
                                .unwrap_or(DEFAULT_PANEL_WIDTH)))
                            .size_range(px(MIN_PANEL_WIDTH)..px(MAX_PANEL_WIDTH))
                            .child(self.feature_panel.clone()),
                    )
                    .child(
                        // Vertical split: editor | terminal
                        v_resizable("editor-terminal")
                            .child(resizable_panel().child(self.feature_editor.clone()))
                            .child(resizable_panel().child(self.terminal_view.clone())),
                    ),
            )
            // Render gpui-component overlay layers (dialogs, sheets, notifications)
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

fn main() {
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx: &mut App| {
            // Load embedded fonts first
            load_embedded_fonts(cx);

            // Initialize gpui-component library (must be called before using any components)
            gpui_component::init(cx);

            // Apply "Pigs in Space" theme colors
            apply_pigs_in_space_theme(cx);

            // Initialize global active feature context
            cx.set_global(ActiveFeatureContext::default());

            // Set up application menus
            set_menus(cx);

            // Bind global keys
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-o", Open, None),
            ]);

            // Register feature editor key bindings
            feature_editor::register_bindings(cx);

            // Register feature panel key bindings
            feature_panel::register_bindings(cx);

            let config = AppConfig::load();
            let window_size = size(
                px(config.window_width.unwrap_or(1200.0)),
                px(config.window_height.unwrap_or(800.0)),
            );

            // Open the main window
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(100.0), px(100.0)),
                    size: window_size,
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some("Manifest".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                focus: true,
                show: true,
                kind: gpui::WindowKind::Normal,
                is_movable: true,
                app_id: Some("com.manifest.app".into()),
                ..Default::default()
            };

            // Create window with Root as the top-level view (required by gpui-component)
            let _window_handle = cx
                .open_window(window_options, |window, cx| {
                    // Create the ManifestApp view
                    let app_view = cx.new(|cx| ManifestApp::new(config, window, cx));
                    // Wrap it in Root (required for gpui-component's theme and overlay system)
                    cx.new(|cx| Root::new(app_view, window, cx))
                })
                .expect("Failed to open window");

            // Register global actions
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &OpenRecent, _cx| {
                eprintln!("Open Recent action triggered");
            });

            // Open action shows directory picker
            // Note: We can't easily update ManifestApp from here without the Entity handle
            // For now, this is a placeholder - proper implementation would store the Entity
            cx.on_action(move |_: &Open, cx| {
                let options = PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Open Manifest Project".into()),
                };
                let _paths_receiver = cx.prompt_for_paths(options);
                // TODO: Store Entity<ManifestApp> to update from here
                eprintln!("Open action - directory picker shown");
            });

            cx.activate(true);
        });
}
