//! Left sprites panel: tree of folders and sprites, drag & drop target.

use super::{App, LogKind};
use eframe::egui;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tp_core::ingest::{is_image_file, normalize_path};
use tp_core::ProjectConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Normal,
    Smart,
    Nested,
}

struct TreeNode {
    path: PathBuf,
    name: String,
    is_dir: bool,
    origin: Origin,
    children: Vec<TreeNode>,
}

enum TreeAction {
    Select(PathBuf, bool),
    /// Exclude a file, or every image inside a directory.
    Remove(PathBuf),
    /// Drop a smart folder from the project.
    RemoveSmart(PathBuf),
    CopyPath(PathBuf),
}

pub(super) fn sprites_ui(app: &mut App, ui: &mut egui::Ui) {
    handle_drop(app, ui);
    let panel_hovered = ui.rect_contains_pointer(ui.max_rect());
    let filter_focused = app.tree_filter_focused;

    let tree = build_tree(&app.config, &app.tree_filter);
    let total = count_files(&tree);

    ui.horizontal(|ui| {
        ui.strong(format!("Sprites ({total})"));
        let hidden = app.hidden_count();
        if hidden > 0 {
            ui.separator();
            if ui
                .small_button(format!("Restaurar ({hidden})"))
                .on_hover_text("Volver a incluir los sprites quitados")
                .clicked()
            {
                app.restore_excluded();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("-")
                .on_hover_text("Plegar todas las carpetas")
                .clicked()
            {
                app.tree_force_open = Some(false);
            }
            if ui
                .small_button("+")
                .on_hover_text("Desplegar todas las carpetas")
                .clicked()
            {
                app.tree_force_open = Some(true);
            }
        });
    });
    let filter_response = ui.add(
        egui::TextEdit::singleline(&mut app.tree_filter)
            .hint_text("Filtrar sprites…")
            .desired_width(f32::INFINITY),
    );
    app.tree_filter_focused = filter_response.has_focus();
    ui.separator();

    // Delete removes the selected sprites (unless the filter is being edited).
    if panel_hovered && !filter_focused && !app.selected_paths.is_empty() {
        let delete = ui.input(|i| i.key_pressed(egui::Key::Delete));
        if delete {
            app.remove_selected();
        }
    }

    if tree.is_empty() {
        ui.add_space(8.0);
        let msg = if app.tree_filter.is_empty() {
            "Arrastra sprites o carpetas aquí\no usa «Añadir sprites» en la barra de herramientas."
        } else {
            "Ningún sprite coincide con el filtro."
        };
        ui.label(egui::RichText::new(msg).weak());
        return;
    }

    let force = app.tree_force_open.take();
    let mut action: Option<TreeAction> = None;
    egui::ScrollArea::vertical()
        .id_salt("sprites_tree")
        .show(ui, |ui| {
            for node in &tree {
                render_node(app, ui, node, true, &mut action, force);
            }
        });

    match action {
        Some(TreeAction::Select(path, toggle)) => apply_selection(app, path, toggle),
        Some(TreeAction::Remove(path)) => {
            let removed = app.remove_path(&path);
            app.selected_paths.remove(&path);
            app.log(LogKind::Info, format!("{removed} sprite(s) quitado(s)."));
        }
        Some(TreeAction::RemoveSmart(dir)) => app.remove_smart_folder(&dir),
        Some(TreeAction::CopyPath(path)) => {
            ui.ctx().copy_text(path.display().to_string());
            app.log(LogKind::Info, format!("Ruta copiada: {}", path.display()));
        }
        None => {}
    }
}

fn render_node(
    app: &App,
    ui: &mut egui::Ui,
    node: &TreeNode,
    root: bool,
    action: &mut Option<TreeAction>,
    force: Option<bool>,
) {
    if node.is_dir {
        let name = match node.origin {
            Origin::Smart => egui::RichText::new(node.name.clone())
                .strong()
                .color(egui::Color32::from_rgb(230, 200, 60)),
            Origin::Nested => egui::RichText::new(node.name.clone())
                .strong()
                .color(egui::Color32::from_rgb(120, 170, 255)),
            Origin::Normal => egui::RichText::new(node.name.clone()).strong(),
        };
        if let Some(open) = force {
            // CollapsingHeader::show() calcula su id dentro de un ui.vertical(...)
            // (hijo con salt por defecto); replicamos ese scope para que el id
            // persistido coincida con el que la cabecera lee después.
            let ctx = ui.ctx().clone();
            ui.vertical(|ui| {
                let id = ui.make_persistent_id(egui::Id::new(("tree_dir", node.path.clone())));
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    &ctx, id, open,
                );
                state.set_open(open);
                state.store(&ctx);
            });
        }
        let inner = egui::CollapsingHeader::new(name)
            .id_salt(("tree_dir", node.path.clone()))
            .default_open(root || !app.tree_filter.is_empty())
            .show(ui, |ui| {
                for child in &node.children {
                    render_node(app, ui, child, false, action, force);
                }
            });
        inner.header_response.context_menu(|ui| {
            dir_menu(ui, node, action);
        });
    } else {
        let name =
            match node.origin {
                Origin::Smart => egui::RichText::new(node.name.clone())
                    .color(egui::Color32::from_rgb(230, 200, 60)),
                Origin::Nested => egui::RichText::new(node.name.clone())
                    .color(egui::Color32::from_rgb(120, 170, 255)),
                Origin::Normal => egui::RichText::new(node.name.clone()),
            };
        let selected = app.selected_paths.contains(&node.path);
        let response = ui.selectable_label(selected, name);
        if response.clicked() {
            let toggle = ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
            *action = Some(TreeAction::Select(node.path.clone(), toggle));
        }
        response.clone().context_menu(|ui| {
            if ui.button("Quitar sprite").clicked() {
                *action = Some(TreeAction::Remove(node.path.clone()));
                ui.close();
            }
            if ui.button("Copiar ruta").clicked() {
                *action = Some(TreeAction::CopyPath(node.path.clone()));
                ui.close();
            }
        });
        response.on_hover_text(node.path.display().to_string());
    }
}

fn dir_menu(ui: &mut egui::Ui, node: &TreeNode, action: &mut Option<TreeAction>) {
    if node.origin == Origin::Smart && ui.button("Quitar carpeta inteligente").clicked() {
        *action = Some(TreeAction::RemoveSmart(node.path.clone()));
        ui.close();
    }
    if ui.button("Quitar carpeta (todos los sprites)").clicked() {
        *action = Some(TreeAction::Remove(node.path.clone()));
        ui.close();
    }
    if ui.button("Copiar ruta").clicked() {
        *action = Some(TreeAction::CopyPath(node.path.clone()));
        ui.close();
    }
}

fn apply_selection(app: &mut App, path: PathBuf, toggle: bool) {
    if toggle {
        if !app.selected_paths.remove(&path) {
            app.selected_paths.insert(path);
        }
    } else {
        app.selected_paths.clear();
        app.selected_paths.insert(path);
    }
    if app.selected_paths.len() == 1 {
        let only = app.selected_paths.iter().next().cloned();
        let id = only.and_then(|p| {
            app.result.as_ref().and_then(|out| {
                out.result
                    .sprites
                    .iter()
                    .find(|s| Path::new(&s.source_path) == p.as_path())
                    .map(|s| s.id.clone())
            })
        });
        app.selected_sprite = id;
    } else {
        app.selected_sprite = None;
    }
}

fn handle_drop(app: &mut App, ui: &mut egui::Ui) {
    let (hovered, dropped) = ui
        .ctx()
        .input(|i| (i.raw.hovered_files.clone(), i.raw.dropped_files.clone()));
    if !ui.rect_contains_pointer(ui.max_rect()) {
        return;
    }
    if !hovered.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(120, 220, 120),
            "Suelta aquí sprites o carpetas",
        );
    }
    if dropped.is_empty() {
        return;
    }
    let mut added = 0;
    for file in dropped {
        if let Some(path) = file.path {
            if app.add_input(path) {
                added += 1;
            }
        }
    }
    if added > 0 {
        app.log(LogKind::Info, format!("{added} sprite(s) añadido(s)."));
    } else {
        app.log(LogKind::Warning, "No se añadieron sprites nuevos.".into());
    }
}

fn matches_filter(name: &str, filter: &str) -> bool {
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

fn build_tree(config: &ProjectConfig, filter: &str) -> Vec<TreeNode> {
    let excluded: HashSet<PathBuf> = config
        .excluded_inputs
        .iter()
        .map(|p| normalize_path(p))
        .collect();
    let mut roots = Vec::new();

    if !config.input_directory.as_os_str().is_empty() && config.input_directory.is_dir() {
        if let Some(node) = dir_node(
            &config.input_directory,
            Origin::Normal,
            config.recursive,
            &excluded,
            filter,
        ) {
            roots.push(node);
        }
    }
    for extra in &config.extra_inputs {
        if extra.is_dir() {
            if let Some(node) = dir_node(extra, Origin::Smart, true, &excluded, filter) {
                roots.push(node);
            }
        } else if extra.is_file()
            && is_image_file(extra)
            && !excluded.contains(&normalize_path(extra))
            && matches_filter(&name_of(extra), filter)
        {
            roots.push(leaf(extra, Origin::Normal));
        }
    }
    roots
}

fn dir_node(
    path: &Path,
    origin: Origin,
    recursive: bool,
    excluded: &HashSet<PathBuf>,
    filter: &str,
) -> Option<TreeNode> {
    let name = name_of(path);
    // When the folder itself matches, its whole content is kept.
    let child_filter = if matches_filter(&name, filter) {
        ""
    } else {
        filter
    };

    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();

    let mut children = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            if recursive {
                let child_origin = match origin {
                    Origin::Smart => Origin::Nested,
                    other => other,
                };
                if let Some(node) =
                    dir_node(&entry, child_origin, recursive, excluded, child_filter)
                {
                    children.push(node);
                }
            }
        } else if is_image_file(&entry)
            && !excluded.contains(&normalize_path(&entry))
            && matches_filter(&name_of(&entry), child_filter)
        {
            let child_origin = match origin {
                Origin::Smart => Origin::Nested,
                other => other,
            };
            children.push(leaf(&entry, child_origin));
        }
    }
    children.retain(|c| !c.is_dir || !c.children.is_empty());
    if children.is_empty() {
        return None;
    }
    Some(TreeNode {
        path: path.to_path_buf(),
        name,
        is_dir: true,
        origin,
        children,
    })
}

fn leaf(path: &Path, origin: Origin) -> TreeNode {
    TreeNode {
        path: path.to_path_buf(),
        name: name_of(path),
        is_dir: false,
        origin,
        children: Vec::new(),
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn count_files(nodes: &[TreeNode]) -> usize {
    nodes
        .iter()
        .map(|n| {
            if n.is_dir {
                count_files(&n.children)
            } else {
                1
            }
        })
        .sum()
}
