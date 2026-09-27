//! Left sprites panel: tree of folders and sprites, drag & drop target.

use super::{App, LogKind};
use eframe::egui;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tp_core::ingest::{is_image_file, normalize_path};
use tp_core::types::SpriteAsset;
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
    /// Assign every sprite under `path` to the group with that name.
    AssignToGroup(PathBuf, String),
    /// Assign concrete sprite ids to the sheet at that index (drag & drop).
    AssignIds(Vec<String>, usize),
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

    groups_ui(app, ui);

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
    let mut drop_target: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt("sprites_tree")
        .show(ui, |ui| {
            // Modelo TexturePacker: hojas (sheets) como nodos del panel, con
            // sus sprites anidados; el arrastre entre hojas reasigna.
            if !app.groups_active() && app.config.folder_groups.len() > 1 {
                for (gi, g) in app.config.folder_groups.iter().enumerate() {
                    render_sheet(app, ui, gi, g, force, &mut action);
                }
                ui.separator();
            }
            for node in &tree {
                render_node(app, ui, node, true, &mut action, force, &mut drop_target);
            }
        });

    match action {
        Some(TreeAction::Select(path, toggle)) => apply_selection(app, path, toggle),
        Some(TreeAction::Remove(path)) => {
            let removed = app.remove_path(&path);
            app.selected_paths.remove(&path);
            app.log(LogKind::Info, format!("{removed} sprite(s) quitado(s)."));
            app.after_workspace_change();
        }
        Some(TreeAction::RemoveSmart(dir)) => app.remove_smart_folder(&dir),
        Some(TreeAction::AssignIds(ids, sheet_index)) => {
            let name = app
                .config
                .folder_groups
                .get(sheet_index)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let moved = app.move_sprites_to_group(&ids, sheet_index);
            if moved > 0 {
                let shown = if name.is_empty() {
                    "hoja principal".to_string()
                } else {
                    name
                };
                app.log(
                    LogKind::Info,
                    format!("{moved} sprite(s) movidos a «{shown}»."),
                );
            }
        }
        Some(TreeAction::AssignToGroup(path, group)) => {
            let ids: Vec<String> = collect_sprite_ids(app, &path);
            let Some(index) = app
                .config
                .folder_groups
                .iter()
                .position(|g| g.name == group)
            else {
                return;
            };
            let mut moved = 0usize;
            for id in ids {
                for (i, g) in app.config.folder_groups.iter_mut().enumerate() {
                    let before = g.sprites.len();
                    g.sprites.retain(|s| s != &id);
                    moved += before - g.sprites.len();
                    if i == index && before == g.sprites.len() {
                        g.sprites.push(id.clone());
                        moved += 1;
                    }
                }
            }
            if moved > 0 {
                app.log(
                    LogKind::Info,
                    format!("{moved} sprite(s) movidos al grupo «{group}»."),
                );
                app.after_workspace_change();
            }
        }
        Some(TreeAction::CopyPath(path)) => {
            ui.ctx().copy_text(path.display().to_string());
            app.log(LogKind::Info, format!("Ruta copiada: {}", path.display()));
        }
        None => {}
    }

    // Soltar un sprite arrastrado (payload SpriteAsset) sobre una carpeta
    // reasigna sus sprites al grupo con el nombre de la carpeta. El payload
    // solo se consume en el frame en que se suelta el botón.
    let released = ui.input(|i| i.pointer.any_released());
    if released && drop_target.is_some() {
        if let Some(sprite) = egui::DragAndDrop::take_payload::<SpriteAsset>(ui.ctx()) {
            let target = drop_target.take().unwrap();
            if let Some(index) = app
                .config
                .folder_groups
                .iter()
                .position(|g| g.name == target)
            {
                let id = sprite.id.clone();
                let mut moved = 0usize;
                for (i, g) in app.config.folder_groups.iter_mut().enumerate() {
                    let before = g.sprites.len();
                    g.sprites.retain(|s| s != &id);
                    moved += before - g.sprites.len();
                    if i == index && before == g.sprites.len() {
                        g.sprites.push(id.clone());
                        moved += 1;
                    }
                }
                if moved > 0 {
                    app.log(
                        LogKind::Info,
                        format!("«{}» movido al grupo «{target}».", sprite.id),
                    );
                    app.after_workspace_change();
                }
            }
        }
    }
}

/// Every image file under `path` (itself included), as ingested sprite ids.
fn collect_sprite_ids(app: &App, path: &Path) -> Vec<String> {
    let Some(out) = &app.result else {
        return Vec::new();
    };
    let by_norm: std::collections::HashMap<PathBuf, &str> = out
        .result
        .sprites
        .iter()
        .map(|s| (normalize_path(Path::new(&s.source_path)), s.id.as_str()))
        .collect();
    let mut ids = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                for e in entries.flatten() {
                    stack.push(e.path());
                }
            }
        } else if is_image_file(&p) {
            if let Some(id) = by_norm.get(&normalize_path(&p)) {
                ids.push((*id).to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// Nodo hoja (sheet) al estilo TexturePacker: contiene sus sprites como
/// hijos, acepta sprites arrastrados desde el árbol o de otras hojas y se
/// puede renombrar/quitar por menú contextual.
fn render_sheet(
    app: &App,
    ui: &mut egui::Ui,
    index: usize,
    group: &tp_core::config::FolderGroup,
    force: Option<bool>,
    action: &mut Option<TreeAction>,
) {
    let id = egui::Id::new(("tree_sheet", group.name.clone()));
    let title = if group.name.is_empty() {
        "(hoja principal)".to_string()
    } else {
        group.name.clone()
    };
    let mut open = group.name.is_empty();
    let header = egui::CollapsingHeader::new(
        egui::RichText::new(format!("🗂 {title} ({})", group.sprites.len())).strong(),
    )
    .id_salt(id)
    .default_open(group.name.is_empty());
    if let Some(f) = force {
        open = f;
    }
    let header = header.open(Some(open));

    let body = header.show(ui, |ui| {
        // Children: sprites de la hoja (por id, resueltos a su ruta).
        let paths: Vec<PathBuf> = group
            .sprites
            .iter()
            .filter_map(|sid| {
                app.result
                    .as_ref()?
                    .result
                    .sprites
                    .iter()
                    .find(|s| &s.id == sid)
                    .map(|s| PathBuf::from(&s.source_path))
            })
            .collect();
        for p in &paths {
            let name = name_of(p);
            let selected = app.selected_paths.contains(p);
            let resp = ui.selectable_label(selected, name);
            if resp.clicked() {
                let toggle =
                    ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
                *action = Some(TreeAction::Select(p.clone(), toggle));
            }
            resp.context_menu(|ui| {
                if ui.button("Quitar sprite").clicked() {
                    *action = Some(TreeAction::Remove(p.clone()));
                    ui.close();
                }
            });
            resp.clone().on_hover_text(p.display().to_string());
            // Drag source: el propio sprite (en modo manual, siempre).
            if !app.config.auto_folder_groups {
                let drag = resp.clone().interact(egui::Sense::drag());
                if drag.dragged() {
                    if let Some(s) = app
                        .result
                        .as_ref()
                        .and_then(|o| {
                            o.result
                                .sprites
                                .iter()
                                .find(|s| Path::new(&s.source_path) == p.as_path())
                        })
                        .cloned()
                    {
                        egui::DragAndDrop::set_payload(ui.ctx(), s);
                    }
                }
            }
        }
        if paths.len() != group.sprites.len() {
            ui.weak("(algunos sprites aún no están cargados)");
        }
        if paths.is_empty() {
            ui.weak("Suelta sprites aquí");
        }
    });

    // Drop zone: cualquier payload de SpriteAsset soltado sobre la hoja.
    let body_hover = body
        .body_response
        .as_ref()
        .is_some_and(|r| r.contains_pointer());
    let over = body.header_response.contains_pointer() || body_hover;
    if egui::DragAndDrop::has_payload_of_type::<SpriteAsset>(ui.ctx()) && over {
        // Marca visual del destino.
        let rect = match body.body_response.as_ref() {
            Some(r) => body.header_response.rect.union(r.rect),
            None => body.header_response.rect,
        };
        ui.painter().rect_filled(
            rect,
            2.0,
            egui::Color32::from_rgba_unmultiplied(120, 200, 120, 40),
        );
    }
    // Al soltar sobre esta hoja: consumir el payload y reasignar a ella.
    if over && ui.input(|i| i.pointer.any_released()) {
        if let Some(sprite) = egui::DragAndDrop::take_payload::<SpriteAsset>(ui.ctx()) {
            *action = Some(TreeAction::AssignIds(vec![sprite.id.clone()], index));
        }
    }
}

fn render_node(
    app: &App,
    ui: &mut egui::Ui,
    node: &TreeNode,
    root: bool,
    action: &mut Option<TreeAction>,
    force: Option<bool>,
    drop_target: &mut Option<String>,
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
                    render_node(app, ui, child, false, action, force, drop_target);
                }
            });
        inner.header_response.context_menu(|ui| {
            dir_menu(ui, node, action, &app.config.folder_groups);
        });
        // Soltar un sprite arrastrado sobre una carpeta reasigna al grupo
        // con el mismo nombre que la carpeta.
        if egui::DragAndDrop::has_payload_of_type::<SpriteAsset>(ui.ctx())
            && inner.header_response.contains_pointer()
        {
            *drop_target = Some(node.name.clone());
        }
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
        response
            .clone()
            .on_hover_text(node.path.display().to_string());
        // Arrastrar un sprite: carga un payload con su id (pack por grupos).
        if app.groups_active() {
            let drag = response.clone().interact(egui::Sense::drag());
            if drag.dragged() {
                let sprite = app
                    .result
                    .as_ref()
                    .and_then(|out| {
                        out.result
                            .sprites
                            .iter()
                            .find(|s| Path::new(&s.source_path) == node.path.as_path())
                    })
                    .cloned();
                if let Some(sprite) = sprite {
                    egui::DragAndDrop::set_payload(ui.ctx(), sprite);
                }
            }
        }
    }
}

fn dir_menu(
    ui: &mut egui::Ui,
    node: &TreeNode,
    action: &mut Option<TreeAction>,
    groups: &[tp_core::config::FolderGroup],
) {
    let has_groups = groups.iter().any(|g| !g.name.is_empty());
    if has_groups {
        ui.menu_button("Mover a grupo…", |ui| {
            for g in groups {
                if g.name.is_empty() {
                    continue;
                }
                if ui.button(g.name.clone()).clicked() {
                    *action = Some(TreeAction::AssignToGroup(node.path.clone(), g.name.clone()));
                    ui.close();
                }
            }
        });
        ui.separator();
    }
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

/// Sección «Hojas» (multipack manual al estilo TexturePacker): añadir hoja,
/// renombrar, vaciar y quitar. Los sprites se asignan arrastrándolos a los
/// nodos de hoja del árbol de arriba.
fn groups_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Hojas (pack por carpetas)")
        .id_salt("output_groups")
        .default_open(false)
        .show(ui, |ui| {
            // Modo automático (estilo TexturePacker original): ignora los
            // grupos manuales y crea un grupo por subcarpeta de entrada.
            let mut auto = app.config.auto_folder_groups;
            if ui
                .checkbox(&mut auto, "Automático por carpetas de entrada")
                .on_hover_text(
                    "Cada subcarpeta de entrada produce su hoja en la subcarpeta de salida \
                     con el mismo nombre; los sprites de la raíz van a la hoja principal",
                )
                .changed()
            {
                app.config.auto_folder_groups = auto;
                app.after_workspace_change();
            }
            if auto {
                ui.weak("Las asignaciones manuales se ignoran en este modo.");
                return;
            }

            let mut remove: Option<usize> = None;
            let mut assign: Option<usize> = None;
            let mut clear: Option<usize> = None;

            for (i, g) in app.config.folder_groups.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if g.name.is_empty() {
                        ui.strong("(hoja principal)");
                    } else {
                        if ui
                            .small_button("✕")
                            .on_hover_text(
                                "Quitar este grupo (sus sprites vuelven a la hoja principal)",
                            )
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut g.name)
                                .desired_width(120.0)
                                .hint_text("subcarpeta de salida"),
                        );
                    }
                    ui.label(format!("({})", g.sprites.len()))
                        .on_hover_text("Sprites asignados a este grupo");
                    if !g.name.is_empty()
                        && !app.selected_paths.is_empty()
                        && ui
                            .small_button("← Selección")
                            .on_hover_text("Mover los sprites seleccionados a este grupo")
                            .clicked()
                    {
                        assign = Some(i);
                    }
                    if !g.name.is_empty()
                        && !g.sprites.is_empty()
                        && ui
                            .small_button("Vaciar")
                            .on_hover_text("Devolver todos sus sprites a la hoja principal")
                            .clicked()
                    {
                        clear = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                let name = app.config.folder_groups[i].name.clone();
                app.config.folder_groups.remove(i);
                app.log(LogKind::Info, format!("Grupo «{name}» eliminado."));
                app.after_workspace_change();
            }
            if let Some(i) = clear {
                app.config.folder_groups[i].sprites.clear();
                app.after_workspace_change();
            }
            if let Some(i) = assign {
                let moved = app.assign_selected_to_group(i);
                if moved > 0 {
                    let name = app.config.folder_groups[i].name.clone();
                    app.log(
                        LogKind::Info,
                        format!("{moved} sprite(s) movidos al grupo «{name}»."),
                    );
                }
            }

            ui.add_space(4.0);
            if ui
                .button("+ Añadir hoja")
                .on_hover_text(
                    "Crea otra hoja: se escribirá en su subcarpeta de salida. \
                     Arrastra sprites al nodo de la hoja para llenarla",
                )
                .clicked()
            {
                let mut n = 1;
                let name = loop {
                    let candidate = format!("hoja{n}");
                    if !app.config.folder_groups.iter().any(|g| g.name == candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                app.config.folder_groups.push(tp_core::config::FolderGroup {
                    name: name.clone(),
                    sprites: Vec::new(),
                });
                app.log(LogKind::Info, format!("Hoja «{name}» creada."));
            }
            ui.weak(
                "Arrastra sprites del árbol (o entre hojas) para moverlos; cada \
                 hoja se escribe en su subcarpeta de salida.",
            );
        });
}
