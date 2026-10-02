//! Left sprites panel: tree of folders and sprites, drag & drop target.

use super::{App, LogKind};
use crate::i18n::t;
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

/// Cómo un clic de fila altera la selección (estándar de gestores de
/// archivos): solo, acumulando o por rango visual desde el ancla.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectMode {
    Replace,
    Toggle,
    Range,
}

impl SelectMode {
    /// Modo resultante de los modificadores pulsados al pulsar la fila.
    fn from_modifiers(mods: egui::Modifiers) -> Self {
        if mods.shift {
            SelectMode::Range
        } else if mods.command || mods.ctrl {
            SelectMode::Toggle
        } else {
            SelectMode::Replace
        }
    }
}

struct TreeNode {
    path: PathBuf,
    name: String,
    is_dir: bool,
    origin: Origin,
    children: Vec<TreeNode>,
}

enum TreeAction {
    Select(PathBuf, SelectMode),
    /// Exclude a file, or every image inside a directory.
    Remove(PathBuf),
    /// Drop a smart folder from the project.
    RemoveSmart(PathBuf),
    /// Assign every sprite under `path` to the group with that name.
    AssignToGroup(PathBuf, String),
    /// Assign concrete sprite ids to the sheet at that index (drag & drop).
    AssignIds(Vec<String>, usize),
    /// Assign the sprite at this path to the sheet at that index (menú).
    AssignPathToSheet(PathBuf, usize),
    CopyPath(PathBuf),
}

/// Estado mutable del recorrido del árbol: acciones pendientes, destino de
/// soltado bajo el cursor y rects en pantalla de las filas de sprite
/// dibujadas este frame (registro para pruebas con eventos de puntero).
struct TreeWalk {
    action: Option<TreeAction>,
    drop_target: Option<String>,
    rows: Vec<(PathBuf, egui::Rect)>,
}

pub(super) fn sprites_ui(app: &mut App, ui: &mut egui::Ui) {
    // El soltar de ficheros del SO lo gestiona la ventana completa
    // (handle_global_file_drop); aquí solo queda el arrastre interno
    // de sprites hacia las hojas.
    let panel_hovered = ui.rect_contains_pointer(ui.max_rect());
    let filter_focused = app.tree_filter_focused;
    // Registro fresco cada frame: las filas que no se dibujen este frame
    // (filtro, panel colapsado…) desaparecen del registro.
    app.sprite_rows.clear();
    // Mientras se escribe en el filtro, las flechas vuelven al texto y la
    // lista deja de moverse por teclado.
    if filter_focused {
        app.tree_kb_focus = false;
    }

    let tree = build_tree(&app.config, &app.tree_filter);
    let total = count_files(&tree);

    ui.horizontal(|ui| {
        ui.strong(t!("Sprites ({})", total));
        let hidden = app.hidden_count();
        if hidden > 0 {
            ui.separator();
            if ui
                .small_button(t!("↺ Restaurar ({})", hidden))
                .on_hover_text(t!("Volver a incluir los sprites quitados"))
                .clicked()
            {
                app.restore_excluded();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("−")
                .on_hover_text(t!("Plegar todas las carpetas"))
                .clicked()
            {
                app.tree_force_open = Some(false);
            }
            if ui
                .small_button("+")
                .on_hover_text(t!("Desplegar todas las carpetas"))
                .clicked()
            {
                app.tree_force_open = Some(true);
            }
        });
    });
    let filter_response = ui.add(
        egui::TextEdit::singleline(&mut app.tree_filter)
            .hint_text(t!("Filtrar sprites…"))
            .desired_width(f32::INFINITY),
    );
    app.tree_filter_focused = filter_response.has_focus();
    ui.separator();

    groups_ui(app, ui);

    if tree.is_empty() {
        ui.add_space(8.0);
        let msg = if app.tree_filter.is_empty() {
            t!("Arrastra imágenes o carpetas\ndesde tu sistema a cualquier\nparte de la ventana.")
        } else {
            t!("Ningún sprite coincide con el filtro.")
        };
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("📁").size(28.0));
            ui.label(egui::RichText::new(msg).weak());
        });
        return;
    }

    let force = app.tree_force_open.take();
    let mut walk = TreeWalk {
        action: None,
        drop_target: None,
        rows: Vec::new(),
    };
    egui::ScrollArea::vertical()
        .id_salt("sprites_tree")
        // Por defecto la ventana del scroll sigue al contenido: se quedaba
        // más estrecha que el panel (y más baja con pocos sprites), con un
        // hueco hasta su borde. Así ocupa todo el ancho y el alto del panel.
        .auto_shrink([false, false])
        // El arrastre del contenido (drag-to-scroll) roba el arrastre a las
        // filas (drag&drop de sprites hacia las hojas).
        .scroll_source(
            egui::scroll_area::ScrollSource::SCROLL_BAR
                | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
        )
        .show(ui, |ui| {
            // Modelo: hojas (sheets) como nodos del panel, con
            // sus sprites anidados; el arrastre entre hojas reasigna. Se
            // muestran en cuanto hay más de una hoja (activa o no).
            if app.config.folder_groups.len() > 1 {
                for (gi, g) in app.config.folder_groups.iter().enumerate() {
                    render_sheet(app, ui, gi, g, force, &mut walk);
                }
                ui.separator();
            }
            for node in &tree {
                render_node(app, ui, node, true, &mut walk, force);
            }
            // Teclado de la lista (flechas, Ctrl+A, Supr): se atiende aquí,
            // dentro del ScrollArea y con las filas ya registradas, para
            // poder desplazar la vista hasta la fila que mueve el cursor.
            handle_list_keys(app, ui, &walk.rows, panel_hovered, filter_focused);
        });
    app.sprite_rows = walk.rows;

    match walk.action {
        Some(TreeAction::Select(path, toggle)) => apply_selection(app, path, toggle),
        Some(TreeAction::Remove(path)) => {
            let removed = app.remove_path(&path);
            app.selected_paths.remove(&path);
            app.log(LogKind::Info, t!("{} sprite(s) quitado(s).", removed));
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
                    t!("hoja principal").to_string()
                } else {
                    name
                };
                app.log(
                    LogKind::Info,
                    t!("{} sprite(s) movidos a «{}».", moved, shown),
                );
            }
        }
        Some(TreeAction::AssignPathToSheet(path, sheet_index)) => {
            // Vía garantizada de asignación: resolución path → ids con la
            // misma lógica que el menú de directorios.
            let ids: Vec<String> = collect_sprite_ids(app, &path);
            let moved = app.move_sprites_to_group(&ids, sheet_index);
            if moved > 0 {
                let name = app
                    .config
                    .folder_groups
                    .get(sheet_index)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                let shown = if name.is_empty() {
                    t!("hoja principal").to_string()
                } else {
                    name
                };
                app.log(
                    LogKind::Info,
                    t!("{} sprite(s) movidos a «{}».", moved, shown),
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
            let moved = app.move_sprites_to_group(&ids, index);
            if moved > 0 {
                app.log(
                    LogKind::Info,
                    t!("{} sprite(s) movidos al grupo «{}».", moved, group),
                );
            }
        }
        Some(TreeAction::CopyPath(path)) => {
            ui.ctx().copy_text(path.display().to_string());
            app.log(LogKind::Info, t!("Ruta copiada: {}", path.display()));
        }
        None => {}
    }

    // Soltar sprites arrastrados (payload unificado SpriteDrag) sobre una
    // carpeta reasigna TODOS los ids del arrastre al grupo con el nombre de
    // la carpeta. El payload solo se consume en el frame de release.
    let released = ui.input(|i| i.pointer.any_released());
    if released && walk.drop_target.is_some() {
        if let Some(drag) = crate::app::SpriteDrag::take(ui.ctx()) {
            let target = walk.drop_target.take().unwrap();
            if let Some(index) = app
                .config
                .folder_groups
                .iter()
                .position(|g| g.name == target)
            {
                let moved = app.move_sprites_to_group(&drag.ids, index);
                if moved > 0 {
                    app.log(
                        LogKind::Info,
                        t!(
                            "{} sprite(s) movidos al grupo «{}».",
                            drag.ids.len(),
                            target
                        ),
                    );
                }
            }
        }
    }
}

/// Ids a arrastrar para la fila `path`: si la fila pertenece a una
/// multi-selección, viajan todos los seleccionados (en orden estable).
fn drag_ids(app: &App, path: &Path) -> Vec<String> {
    let Some(out) = app.result.as_ref() else {
        return Vec::new();
    };
    let norm = normalize_path(path);
    let dragged_selected = app.selected_paths.iter().any(|p| normalize_path(p) == norm);
    let own = path.to_path_buf();
    let paths: Vec<&PathBuf> = if dragged_selected && app.selected_paths.len() > 1 {
        app.selected_paths.iter().collect()
    } else {
        vec![&own]
    };
    let mut ids: Vec<String> = Vec::new();
    for p in paths {
        let n = normalize_path(p);
        if let Some(s) = out
            .result
            .sprites
            .iter()
            .find(|s| normalize_path(Path::new(&s.source_path)) == n)
        {
            ids.push(s.id.clone());
        }
    }
    ids.sort();
    ids
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

/// Nodo hoja (sheet): contiene sus sprites como
/// hijos, acepta sprites arrastrados desde el árbol o de otras hojas y se
/// puede renombrar/quitar por menú contextual.
fn render_sheet(
    app: &App,
    ui: &mut egui::Ui,
    index: usize,
    group: &tp_core::config::FolderGroup,
    force: Option<bool>,
    walk: &mut TreeWalk,
) {
    let id = egui::Id::new(("tree_sheet", group.name.clone()));
    let title = if group.name.is_empty() {
        t!("(hoja principal)").to_string()
    } else {
        group.name.clone()
    };
    let mut open = group.name.is_empty();
    let header = egui::CollapsingHeader::new(
        egui::RichText::new(t!("📂 {} ({})", title, group.sprites.len())).strong(),
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
            // Recién soltado: el color señala «aquí acaba de entrar».
            let label = if app.is_just_added(p) {
                egui::RichText::new(name.as_str()).color(super::just_added_color(ui.visuals()))
            } else {
                egui::RichText::new(name.as_str())
            };
            let resp = ui.selectable_label(selected, label);
            // Registro para pruebas: rect en pantalla de esta fila.
            walk.rows.push((p.clone(), resp.rect));
            if resp.clicked() {
                let mode = ui.input(|i| SelectMode::from_modifiers(i.modifiers));
                walk.action = Some(TreeAction::Select(p.clone(), mode));
            }
            resp.context_menu(|ui| {
                if ui.button(t!("Quitar sprite")).clicked() {
                    walk.action = Some(TreeAction::Remove(p.clone()));
                    ui.close();
                }
            });
            // Drag source: widget de arrastre con id propio sobre la fila.
            // El payload va al lienzo (colocar donde se suelta) y a las hojas
            // del árbol (pack por carpetas).
            if !app.config.auto_folder_groups {
                // Solo el drag de sprites ya empaquetados (con frame) coloca
                // en el lienzo: los nuevos entran por el flujo automático.
                if app
                    .result
                    .as_ref()
                    .and_then(|o| {
                        o.result
                            .sprites
                            .iter()
                            .find(|s| Path::new(&s.source_path) == p.as_path())
                    })
                    .is_some()
                {
                    // Mismo id que la fila: clic y arrastre conviven.
                    let dnd = resp.interact(egui::Sense::drag());
                    if dnd.dragged() {
                        // Un solo payload: SpriteDrag (hay una única ranura
                        // de DragAndDrop y el doble set lo sobrescribía).
                        crate::app::begin_sprite_drag(app, ui.ctx(), drag_ids(app, p));
                    }
                    dnd.on_hover_cursor(egui::CursorIcon::Grab);
                }
            }
        }
        if paths.len() != group.sprites.len() {
            ui.weak(t!("(algunos sprites aún no están cargados)"));
        }
        if paths.is_empty() {
            ui.weak(t!("Suelta sprites aquí"));
        }
    });

    let body_hover = body
        .body_response
        .as_ref()
        .is_some_and(|r| r.contains_pointer());
    let hover = crate::app::SpriteDrag::has_payload(ui.ctx())
        && (body.header_response.contains_pointer()
            || body
                .body_response
                .as_ref()
                .is_some_and(|r| r.contains_pointer()));
    if hover {
        // Marca visual del destino mientras se arrastra encima.
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
    // Al soltar sobre esta hoja: reasignar los sprites arrastrados a ella.
    // El payload unificado SpriteDrag se consume aquí (mismo destino que el
    // lienzo: la primera zona que lo consuma se queda el arrastre).
    let released = ui.input(|i| i.pointer.any_released());
    let over_sheet = body.header_response.contains_pointer()
        || body
            .body_response
            .as_ref()
            .is_some_and(|r| r.contains_pointer());
    if released && over_sheet && crate::app::SpriteDrag::has_payload(ui.ctx()) {
        if let Some(drag) = crate::app::SpriteDrag::take(ui.ctx()) {
            walk.action = Some(TreeAction::AssignIds(drag.ids.clone(), index));
        }
    }
    let _ = body_hover;
}

fn render_node(
    app: &App,
    ui: &mut egui::Ui,
    node: &TreeNode,
    root: bool,
    walk: &mut TreeWalk,
    force: Option<bool>,
) {
    if node.is_dir {
        let mut name = match node.origin {
            Origin::Smart => egui::RichText::new(node.name.clone())
                .strong()
                .color(super::amber_color(ui.visuals())),
            Origin::Nested => egui::RichText::new(node.name.clone())
                .strong()
                .color(super::info_color(ui.visuals())),
            Origin::Normal => egui::RichText::new(node.name.clone()).strong(),
        };
        if app.is_just_added(&node.path) {
            name = name.color(super::just_added_color(ui.visuals()));
        }
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
                    render_node(app, ui, child, false, walk, force);
                }
            });
        inner.header_response.context_menu(|ui| {
            dir_menu(ui, node, &mut walk.action, &app.config.folder_groups);
        });
        // Soltar sprites arrastrados sobre una carpeta reasigna al grupo
        // con el mismo nombre que la carpeta (payload unificado SpriteDrag).
        if crate::app::SpriteDrag::has_payload(ui.ctx()) && inner.header_response.contains_pointer()
        {
            walk.drop_target = Some(node.name.clone());
        }
    } else {
        let mut name = match node.origin {
            Origin::Smart => {
                egui::RichText::new(node.name.clone()).color(super::amber_color(ui.visuals()))
            }
            Origin::Nested => {
                egui::RichText::new(node.name.clone()).color(super::info_color(ui.visuals()))
            }
            Origin::Normal => egui::RichText::new(node.name.clone()),
        };
        if app.is_just_added(&node.path) {
            name = name.color(super::just_added_color(ui.visuals()));
        }
        let selected = app.selected_paths.contains(&node.path);
        let response = ui.selectable_label(selected, name);
        // Registro para pruebas: rect en pantalla de esta fila.
        walk.rows.push((node.path.clone(), response.rect));
        if response.clicked() {
            let mode = ui.input(|i| SelectMode::from_modifiers(i.modifiers));
            walk.action = Some(TreeAction::Select(node.path.clone(), mode));
        }
        response.clone().context_menu(|ui| {
            // Vía garantizada para asignar a hoja (además del arrastre).
            let sheets: Vec<(usize, String)> = app
                .config
                .folder_groups
                .iter()
                .enumerate()
                .map(|(i, g)| (i, g.name.clone()))
                .collect();
            if sheets.len() > 1 {
                ui.menu_button(t!("Mover a hoja…"), |ui| {
                    for (i, name) in &sheets {
                        let label = if name.is_empty() {
                            t!("(hoja principal)").to_string()
                        } else {
                            name.clone()
                        };
                        if ui.button(label).clicked() {
                            walk.action =
                                Some(TreeAction::AssignPathToSheet(node.path.clone(), *i));
                            ui.close();
                        }
                    }
                });
            }
            if ui.button(t!("Quitar sprite")).clicked() {
                walk.action = Some(TreeAction::Remove(node.path.clone()));
                ui.close();
            }
            if ui.button(t!("Copiar ruta")).clicked() {
                walk.action = Some(TreeAction::CopyPath(node.path.clone()));
                ui.close();
            }
        });
        response
            .clone()
            .on_hover_text(node.path.display().to_string());
        // Arrastrar un sprite del árbol: hacia el lienzo (colocarlo donde
        // se suelta) o hacia una hoja (pack por carpetas).
        if !app.config.auto_folder_groups {
            // Igual arriba: solo sprites ya empaquetados.
            if app
                .result
                .as_ref()
                .and_then(|out| {
                    out.result
                        .sprites
                        .iter()
                        .find(|s| Path::new(&s.source_path) == node.path.as_path())
                })
                .is_some()
            {
                // Mismo id que la fila: los sentidos se fusionan y el clic
                // no se pierde (un overlay con id distinto roba el press).
                let dnd = response.interact(egui::Sense::drag());
                if dnd.dragged() {
                    // Un solo payload: SpriteDrag (el segundo set_payload
                    // sobrescribiría el primero).
                    crate::app::begin_sprite_drag(app, ui.ctx(), drag_ids(app, &node.path));
                }
                dnd.on_hover_cursor(egui::CursorIcon::Grab);
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
        ui.menu_button(t!("Mover a grupo…"), |ui| {
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
    if node.origin == Origin::Smart && ui.button(t!("Quitar carpeta inteligente")).clicked() {
        *action = Some(TreeAction::RemoveSmart(node.path.clone()));
        ui.close();
    }
    if ui
        .button(t!("Quitar carpeta (todos los sprites)"))
        .clicked()
    {
        *action = Some(TreeAction::Remove(node.path.clone()));
        ui.close();
    }
    if ui.button(t!("Copiar ruta")).clicked() {
        *action = Some(TreeAction::CopyPath(node.path.clone()));
        ui.close();
    }
}

fn apply_selection(app: &mut App, path: PathBuf, mode: SelectMode) {
    match mode {
        SelectMode::Replace => {
            app.selected_paths.clear();
            app.selected_paths.insert(path.clone());
        }
        SelectMode::Toggle => {
            if !app.selected_paths.remove(&path) {
                app.selected_paths.insert(path.clone());
            }
        }
        SelectMode::Range => {
            // Rango visual entre el ancla y la fila pulsada, siguiendo el
            // orden real de la lista (las filas registradas este frame).
            let order: Vec<PathBuf> = app.sprite_rows.iter().map(|(p, _)| p.clone()).collect();
            let target = order.iter().position(|p| *p == path);
            let anchor = app
                .selection_anchor
                .clone()
                .or_else(|| app.list_cursor.clone())
                .filter(|a| order.iter().any(|p| p == a));
            match (target, anchor) {
                (Some(t), Some(a)) => {
                    let a = order.iter().position(|p| *p == a).unwrap_or(t);
                    app.selected_paths.clear();
                    for p in &order[a.min(t)..=a.max(t)] {
                        app.selected_paths.insert(p.clone());
                    }
                }
                (Some(t), None) => {
                    app.selected_paths.clear();
                    app.selected_paths.insert(order[t].clone());
                }
                _ => {}
            }
        }
    }
    match mode {
        SelectMode::Replace | SelectMode::Toggle => {
            app.list_cursor = Some(path.clone());
            app.selection_anchor = Some(path);
        }
        // Shift no mueve el ancla: de ella sigue partiendo el rango.
        SelectMode::Range => {
            app.list_cursor = Some(path);
            if app.selection_anchor.is_none() {
                app.selection_anchor = app.list_cursor.clone();
            }
        }
    }
    // El panel gana el foco de teclado: las flechas siguen funcionando
    // aunque el puntero se vaya de la lista.
    app.tree_kb_focus = true;
    app.sync_selected_sprite();
}

/// Teclado de la lista de sprites, atendido dentro del ScrollArea con las
/// filas visibles del propio frame:
///
/// * `↑`/`↓`/`PageUp`/`PageDown`/`Home`/`End` mueven el cursor y dejan la
///   selección en esa fila (la vista sigue al cursor);
/// * `Shift` + esas teclas extienden el rango desde el ancla: selección
///   por lote;
/// * `Ctrl` + esas teclas acumulan la fila a la selección;
/// * `Ctrl+A` selecciona toda la lista visible;
/// * `Supr` quita la selección y deja el cursor en la fila siguiente.
fn handle_list_keys(
    app: &mut App,
    ui: &egui::Ui,
    rows: &[(PathBuf, egui::Rect)],
    panel_hovered: bool,
    filter_focused: bool,
) {
    // Con el filtro activo —o con cualquier otro widget con foco de
    // teclado, p. ej. un campo de Ajustes— las teclas no son de la lista.
    if rows.is_empty()
        || filter_focused
        || ui.ctx().wants_keyboard_input()
        || !(panel_hovered || app.tree_kb_focus)
    {
        return;
    }
    let paths: Vec<PathBuf> = rows.iter().map(|(p, _)| p.clone()).collect();

    if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        app.remove_selected(&paths);
        return;
    }
    // Ctrl+A: el lote entero, en el orden en que se ve la lista.
    if ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::A)) {
        app.selected_paths.clear();
        app.selected_paths.extend(paths.iter().cloned());
        app.list_cursor = paths.last().cloned();
        app.selection_anchor = paths.first().cloned();
        app.tree_kb_focus = true;
        app.sync_selected_sprite();
        return;
    }

    let (down, up, page_down, page_up, home, end, shift, ctrl) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::PageDown),
            i.key_pressed(egui::Key::PageUp),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
            i.modifiers.shift,
            i.modifiers.ctrl || i.modifiers.command,
        )
    });
    if !(down || up || page_down || page_up || home || end) {
        return;
    }

    let page = rows
        .iter()
        .filter(|(_, r)| r.intersects(ui.clip_rect()))
        .count()
        .max(1) as i32;
    let last = paths.len() as i32 - 1;
    let cursor = app
        .list_cursor
        .as_ref()
        .or_else(|| app.selected_paths.iter().next())
        .and_then(|c| paths.iter().position(|p| p == c));
    // Sin cursor previo, ↑/PageUp/End entran por el final de la lista.
    let current = match cursor {
        Some(c) => c as i32,
        None if up || page_up || end => last,
        None => 0,
    };
    let target = if down {
        current + 1
    } else if up {
        current - 1
    } else if page_down {
        current + page
    } else if page_up {
        current - page
    } else if home {
        0
    } else {
        last
    };
    let target = target.clamp(0, last) as usize;
    let path = paths[target].clone();

    if shift {
        let anchor = app
            .selection_anchor
            .clone()
            .or_else(|| app.list_cursor.clone())
            .filter(|a| paths.contains(a));
        match anchor {
            Some(a) => {
                let a = paths.iter().position(|p| *p == a).unwrap_or(target);
                app.selected_paths.clear();
                app.selected_paths
                    .extend(paths[a.min(target)..=a.max(target)].iter().cloned());
                app.selection_anchor = Some(paths[a].clone());
            }
            None => {
                app.selected_paths.clear();
                app.selected_paths.insert(path.clone());
                app.selection_anchor = Some(path.clone());
            }
        }
    } else if ctrl {
        // Acumula sin tocar el resto de la selección.
        app.selected_paths.insert(path.clone());
        if app.selection_anchor.is_none() {
            app.selection_anchor = Some(path.clone());
        }
    } else {
        app.selected_paths.clear();
        app.selected_paths.insert(path.clone());
        app.selection_anchor = Some(path.clone());
    }
    app.list_cursor = Some(path);
    app.tree_kb_focus = true;
    app.sync_selected_sprite();
    // La vista sigue al cursor (mínimo desplazamiento para mostrarlo).
    ui.scroll_to_rect(rows[target].1, None);
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

/// Sección «Hojas» (multipack manual): añadir hoja,
/// renombrar, vaciar y quitar. Los sprites se asignan arrastrándolos a los
/// nodos de hoja del árbol de arriba.
fn groups_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(t!("Hojas (pack por carpetas)"))
        .id_salt("output_groups")
        .default_open(false)
        .show(ui, |ui| {
            // Modo automático: ignora los
            // grupos manuales y crea un grupo por subcarpeta de entrada.
            let mut auto = app.config.auto_folder_groups;
            if ui
                .checkbox(&mut auto, t!("Automático por carpetas de entrada"))
                .on_hover_text(t!(
                    "Cada subcarpeta de entrada produce su hoja en la subcarpeta de salida \
                     con el mismo nombre; los sprites de la raíz van a la hoja principal"
                ))
                .changed()
            {
                app.config.auto_folder_groups = auto;
                app.after_workspace_change();
            }
            if auto {
                ui.weak(t!("Las asignaciones manuales se ignoran en este modo."));
                return;
            }

            let mut remove: Option<usize> = None;
            let mut assign: Option<usize> = None;
            let mut clear: Option<usize> = None;

            for (i, g) in app.config.folder_groups.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if g.name.is_empty() {
                        ui.strong(t!("(hoja principal)"));
                    } else {
                        if ui
                            .small_button("×")
                            .on_hover_text(t!(
                                "Quitar este grupo (sus sprites vuelven a la hoja principal)"
                            ))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut g.name)
                                .desired_width(120.0)
                                .hint_text(t!("subcarpeta de salida")),
                        );
                    }
                    ui.label(t!("({})", g.sprites.len()))
                        .on_hover_text(t!("Sprites asignados a este grupo"));
                    if !g.name.is_empty()
                        && !app.selected_paths.is_empty()
                        && ui
                            .small_button(t!("↪ Selección"))
                            .on_hover_text(t!("Mover los sprites seleccionados a este grupo"))
                            .clicked()
                    {
                        assign = Some(i);
                    }
                    if !g.name.is_empty()
                        && !g.sprites.is_empty()
                        && ui
                            .small_button(t!("Vaciar"))
                            .on_hover_text(t!("Devolver todos sus sprites a la hoja principal"))
                            .clicked()
                    {
                        clear = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                let name = app.config.folder_groups[i].name.clone();
                app.config.folder_groups.remove(i);
                app.log(LogKind::Info, t!("Grupo «{}» eliminado.", name));
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
                        t!("{} sprite(s) movidos al grupo «{}».", moved, name),
                    );
                }
            }

            ui.add_space(4.0);
            if ui
                .button(t!("+ Añadir hoja"))
                .on_hover_text(t!(
                    "Crea otra hoja: se escribirá en su subcarpeta de salida. \
                     Arrastra sprites al nodo de la hoja para llenarla"
                ))
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
                app.log(LogKind::Info, t!("Hoja «{}» creada.", name));
            }
            ui.weak(t!(
                "Arrastra sprites del árbol (o entre hojas) para moverlos; cada \
                 hoja se escribe en su subcarpeta de salida."
            ));
        });
}
