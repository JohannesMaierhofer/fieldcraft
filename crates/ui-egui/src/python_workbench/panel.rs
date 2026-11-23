use std::fs;
use std::path::{Path, PathBuf};

use egui::{Color32, FontId, Key, Modifiers, RichText, TextEdit, TextFormat, Ui, pos2, vec2};
use solidcraft_ui_kit as kit;
use solidcraft_ui_text::tr;

use super::{Document, MAX_FILE, PreferredView, Selection, Workspace, append, in_project, show_result};
use crate::ModelApp;

enum Action {
    Detach(bool),
    Hide,
    PickProject,
    SetProject,
    Open(PathBuf),
    OpenModel(PathBuf),
    Activate(PathBuf),
    Close(usize),
    Save(usize),
    Create(bool),
    Case,
    Result(PathBuf),
    RunFile,
    RunSelection,
    Evaluate,
    LoadMesh,
    Stop,
    Reset,
    Trace(PathBuf, usize),
}

pub(crate) fn show(app: &mut ModelApp, ui: &mut Ui) {
    if !app.python_workspace.open {
        return;
    }
    let ctx = ui.ctx().clone();
    let workspace = &mut app.python_workspace;
    let mut action = None;
    if workspace.detached {
        let id = egui::ViewportId::from_hash_of("fieldcraft_python_workspace");
        let window = egui::ViewportBuilder::default()
            .with_title("FieldCraft — Python Workspace")
            .with_inner_size([900.0, 670.0])
            .with_min_inner_size([500.0, 440.0]);
        ctx.show_viewport_immediate(id, window, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                workspace.panel(ui, &mut action);
            } else if ui.input(|input| input.viewport().close_requested()) {
                action = Some(Action::Detach(false));
            } else {
                egui::CentralPanel::default().show(ui, |ui| workspace.panel(ui, &mut action));
            }
        });
    } else {
        let max_width = (ui.available_width() - 215.0).max(330.0);
        let initial_width = (ui.available_width() * 0.47).min(740.0);
        let width = workspace.dock_width.unwrap_or(initial_width).clamp(330.0, max_width);
        let panel = egui::Panel::right("fc_python_workspace_dock").exact_size(width).resizable(false).show(ui, |ui| {
            let margin = egui::Margin { left: 16, right: 8, top: 2, bottom: 0 };
            egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                ui.set_width((width - 48.0).max(250.0));
                workspace.panel(ui, &mut action);
            });
        });
        let rect = panel.response.rect;
        let divider = egui::Rect::from_center_size(rect.left_center(), vec2(8.0, rect.height()));
        let handle = ui
            .interact(divider, egui::Id::unique("fc_python_workspace_resize"), egui::Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if handle.dragged() {
            let delta = ui.input(|input| input.pointer.delta().x);
            workspace.dock_width = Some((width - delta).clamp(330.0, max_width));
            ctx.request_repaint();
        }
    }
    if let Some(action) = action {
        match perform(app, action) {
            Ok(()) => app.python_workspace.error = None,
            Err(error) => app.python_workspace.error = Some(error),
        }
        ctx.request_repaint();
    }
}

fn perform(app: &mut ModelApp, action: Action) -> Result<(), String> {
    let workspace = &mut app.python_workspace;
    match action {
        Action::Detach(detached) => workspace.set_detached(detached),
        Action::Hide => workspace.open = false,
        Action::PickProject => app.pick_python_project(),
        Action::SetProject => workspace.set_project(&PathBuf::from(&workspace.folder))?,
        Action::Open(path) => workspace.open_file(&path)?,
        Action::OpenModel(path) => app.open_path(super::path_string(&path)?),
        Action::Activate(path) => workspace.activate(path),
        Action::Close(index) => {
            if workspace.tabs.get(index).is_some_and(Document::dirty) {
                return Err("Save the file or discard its edits before closing the tab".into());
            }
            if index < workspace.tabs.len() {
                let tab = workspace.tabs.remove(index);
                if workspace.active.as_ref() == Some(&tab.path) {
                    workspace.active = workspace.tabs.last().map(|tab| tab.path.clone());
                    workspace.selection = None;
                }
            }
        }
        Action::Save(index) => workspace.save(index)?,
        Action::Create(folder) => workspace.create(folder)?,
        Action::Case => workspace.run_case(&app.session)?,
        Action::Result(path) => show_result(app, &path, &PreferredView::default())?,
        Action::RunFile => workspace.run_file()?,
        Action::RunSelection => workspace.run_selection()?,
        Action::Evaluate => {
            let input = workspace.console_input.clone();
            workspace.console_request(&input, "<console>", 1, None)?;
            workspace.console_input.clear();
        }
        Action::LoadMesh => workspace.use_run_in_console()?,
        Action::Stop => workspace.stop_job(),
        Action::Reset => {
            workspace.console = None;
            append(&mut workspace.log, "Reset Python console".into());
        }
        Action::Trace(path, line) => {
            workspace.open_file(&path)?;
            workspace.jump_line = Some(line);
        }
    }
    Ok(())
}

pub(crate) fn settings_page(ui: &mut Ui, app: &mut ModelApp) {
    kit::settings_page(ui, |ui| {
        kit::settings_heading(ui, tr("settings.python"), tr("settings.python_note"));
        kit::settings_section(ui, "python-runtime", tr("settings.python_runtime"), |ui| {
            let mut python = app.prefs.python_interpreter.clone();
            let mut bridge = app.prefs.python_bridge.clone();
            let automatic = tr("settings.python_automatic");
            let python_changed = kit::row(ui, tr("settings.python_interpreter"), |ui| kit::text_field(ui, &mut python, automatic).1);
            let bridge_changed = kit::row(ui, tr("settings.python_bridge"), |ui| kit::text_field(ui, &mut bridge, automatic).1);
            if python_changed || bridge_changed {
                app.set_python_paths(python, bridge);
            }
            let workspace = &app.python_workspace;
            if let Some(runtime) = &workspace.runtime {
                let interpreter = workspace.project.as_deref().map_or_else(|| PathBuf::from(&runtime.python), |root| runtime.python(root));
                let paths =
                    [(tr("settings.python_active"), interpreter.as_path()), (tr("settings.python_active_bridge"), Path::new(&runtime.bridge))];
                for (label, path) in paths {
                    let value = if path.as_os_str().is_empty() { tr("settings.python_missing").into() } else { path.display().to_string() };
                    ui.add(egui::Label::new(format!("{label}: {value}")).wrap());
                }
            }
        });
        kit::settings_section(ui, "python-packages", tr("settings.python_packages"), |ui| {
            let workspace = &mut app.python_workspace;
            let Some(project) = &workspace.project else {
                kit::text(ui, tr("settings.python_choose_project"), kit::Text::Note("settings.python_choose_project"));
                return;
            };
            ui.add(egui::Label::new(project.display().to_string()).wrap());
            ui.add_enabled_ui(workspace.job.is_none(), |ui| {
                let submit = kit::row(ui, tr("settings.python_package"), |ui| kit::text_field(ui, &mut workspace.package, "scipy").0);
                ui.horizontal_wrapped(|ui| {
                    let sync = kit::button(ui, tr("settings.python_sync"), kit::Button::Normal).clicked();
                    let add = kit::button(ui, tr("settings.python_add"), kit::Button::Normal).clicked() || submit;
                    if (add || sync)
                        && let Err(error) = workspace.uv(add)
                    {
                        workspace.environment_status = Some(Err(error));
                    }
                });
            });
            if let Some(status) = &workspace.environment_status {
                match status {
                    Ok(text) => ui.label(text),
                    Err(error) => ui.colored_label(ui.visuals().error_fg_color, error),
                };
            }
        });
    });
}

impl Workspace {
    fn panel(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        ui.horizontal(|ui| {
            if self.detached {
                if ui.button("Dock").clicked() {
                    *action = Some(Action::Detach(false));
                }
            } else {
                ui.heading("Python Workspace");
                if ui.button("Undock").clicked() {
                    *action = Some(Action::Detach(true));
                }
                if ui.button("Close").clicked() {
                    *action = Some(Action::Hide);
                }
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("Project");
            let response = ui.add(TextEdit::singleline(&mut self.folder).desired_width((ui.available_width() - 166.0).max(115.0)));
            if ui.button("Browse…").clicked() {
                *action = Some(Action::PickProject);
            }
            if ui.button("Open Folder").clicked() || response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                *action = Some(Action::SetProject);
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(root) = self.project.clone() else { return };
        ui.separator();
        self.run_bar(ui, action);
        ui.separator();
        let height = (ui.available_height() - 172.0).clamp(100.0, 510.0);
        let explorer_width = (ui.available_width() * 0.32).clamp(115.0, 200.0);
        ui.horizontal_top(|ui| {
            let layout = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(vec2(explorer_width, height), layout, |ui| self.explorer(ui, &root, explorer_width, height, action));
            let (divider, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
            ui.painter().vline(divider.center().x, divider.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
            self.editor(ui, height, action);
        });
        ui.separator();
        self.console(ui, &root, action);
    }

    fn run_bar(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        ui.horizontal_wrapped(|ui| {
            let idle = self.job.is_none();
            let case = self.case.as_deref().map_or("Open a .case.json file", filename);
            if ui.add_enabled(idle && self.case.is_some(), egui::Button::new("Run Case")).on_hover_text(case).clicked() {
                *action = Some(Action::Case);
            }
            ui.checkbox(&mut self.use_open_model, "Use Open Model");
            if ui.add_enabled(!idle, egui::Button::new("Stop Job")).clicked() {
                *action = Some(Action::Stop);
            }
            if !idle {
                ui.spinner();
            }
        });
    }

    fn explorer(&mut self, ui: &mut Ui, root: &Path, width: f32, height: f32, action: &mut Option<Action>) {
        ui.set_width(width);
        ui.heading("Files");
        ui.add(TextEdit::singleline(&mut self.new_path).hint_text("New file or folder path").desired_width((width - 10.0).max(80.0)));
        ui.horizontal_wrapped(|ui| {
            if ui.button("New File").clicked() {
                *action = Some(Action::Create(false));
            }
            if ui.button("New Folder").clicked() {
                *action = Some(Action::Create(true));
            }
        });
        egui::ScrollArea::both().id_salt("fc_python_files").max_height((height - 72.0).max(60.0)).show(ui, |ui| {
            tree(ui, root, self.active.as_deref(), 0, &mut 600, action);
        });
    }

    fn editor(&mut self, ui: &mut Ui, height: f32, action: &mut Option<Action>) {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                for tab in &self.tabs {
                    let label = format!("{}{}", filename(&tab.path), if tab.dirty() { " *" } else { "" });
                    if ui.selectable_label(self.active.as_ref() == Some(&tab.path), label).on_hover_text(tab.path.display().to_string()).clicked() {
                        *action = Some(Action::Activate(tab.path.clone()));
                    }
                }
            });
            let Some(index) = self.active_tab() else {
                ui.label("Open a Python script or case from Files.");
                return;
            };
            let Some(tab) = self.tabs.get_mut(index) else { return };
            let id = egui::Id::unique(("fc_python_editor", &tab.path));
            let python = tab.path.extension().and_then(|ext| ext.to_str()) == Some("py");
            let selected = self.selection.as_ref().is_some_and(|selection| selection.start != selection.end);
            let console_idle = self.console.as_ref().is_none_or(|console| !console.busy);
            let has_focus = ui.memory(|memory| memory.has_focus(id));
            let mut height = height - 51.0;
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(tab.dirty(), egui::Button::new("Save")).clicked()
                    || has_focus && ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::S))
                {
                    *action = Some(Action::Save(index));
                }
                if ui.button("Close Tab").clicked() {
                    *action = Some(Action::Close(index));
                }
                if tab.dirty() && ui.button("Discard Edits").clicked() {
                    tab.text.clone_from(&tab.saved);
                    self.selection = None;
                }
                if ui.add_enabled(python && self.job.is_none(), egui::Button::new("Run Python File")).clicked() {
                    *action = Some(Action::RunFile);
                }
                if ui.add_enabled(python && selected && console_idle, egui::Button::new("Run Selection")).on_hover_text("Ctrl/Cmd+Enter").clicked()
                    || python && selected && console_idle && has_focus && ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Enter))
                {
                    *action = Some(Action::RunSelection);
                }
                if filename(&tab.path) == "results.json" && ui.button("Show Results").clicked() {
                    *action = Some(Action::Result(tab.path.clone()));
                }
            });
            if tab.path.extension().and_then(|ext| ext.to_str()) == Some("csv") && plot_csv(ui, &tab.text) {
                height -= 112.0;
            }
            let editor_width = ui.available_width();
            egui::ScrollArea::both().id_salt(("fc_python_scroll", &tab.path)).max_height(height.max(75.0)).show(ui, |ui| {
                let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _: f32| highlight(ui, buffer.as_str(), python);
                let rows = tab.text.lines().count().clamp(7, 5000);
                if let Some(line) = self.jump_line.take() {
                    let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                    let offset: usize = tab.text.split_inclusive('\n').take(line.saturating_sub(1)).map(|row| row.chars().count()).sum();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(offset))));
                    state.store(ui.ctx(), id);
                    ui.memory_mut(|memory| memory.request_focus(id));
                }
                ui.horizontal_top(|ui| {
                    let font = FontId::monospace(13.0);
                    let numbers = (1..=rows).map(|row| format!("{row:>4}\n")).collect::<String>();
                    ui.add_space(2.0);
                    ui.label(RichText::new(numbers).font(font).weak());
                    let editor = TextEdit::multiline(&mut tab.text)
                        .id(id)
                        .code_editor()
                        .desired_rows(rows)
                        .desired_width((editor_width - 60.0).max(160.0))
                        .char_limit(MAX_FILE as usize)
                        .layouter(&mut layouter)
                        .show(ui);
                    if let Some(range) = editor.cursor_range {
                        let [start, end] = range.sorted_cursors();
                        self.selection = Some(Selection { start: start.index.into(), end: end.index.into() });
                    }
                });
            });
        });
    }

    fn console(&mut self, ui: &mut Ui, root: &Path, action: &mut Option<Action>) {
        let busy = self.console.as_ref().is_some_and(|console| console.busy);
        ui.horizontal(|ui| {
            ui.heading("Console");
            if ui.add_enabled(self.console.is_some(), egui::Button::new("Reset Console")).clicked() {
                *action = Some(Action::Reset);
            }
            if ui.add_enabled(!busy && self.last_result.is_some(), egui::Button::new("Load Last Case Mesh")).clicked() {
                *action = Some(Action::LoadMesh);
            }
            if busy {
                ui.spinner();
            }
        });
        egui::ScrollArea::both().id_salt("fc_python_console_log").stick_to_bottom(true).max_height(105.0).show(ui, |ui| {
            ui.set_min_height(85.0);
            for line in &self.log {
                if let Some((path, number)) = frame(line).and_then(|(path, number)| in_project(root, Path::new(path)).ok().map(|path| (path, number)))
                {
                    if ui.link(line).clicked() {
                        *action = Some(Action::Trace(path, number));
                    }
                } else {
                    ui.label(RichText::new(line).monospace());
                }
            }
        });
        ui.horizontal(|ui| {
            let entry = ui.add_enabled(
                !busy,
                TextEdit::singleline(&mut self.console_input)
                    .hint_text("Python expression or statement")
                    .desired_width((ui.available_width() - 55.0).max(130.0)),
            );
            if ui.add_enabled(!busy, egui::Button::new("Run")).clicked() || entry.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                *action = Some(Action::Evaluate);
                entry.request_focus();
            }
        });
    }
}

fn filename(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("Untitled")
}

fn tree(ui: &mut Ui, dir: &Path, active: Option<&Path>, depth: usize, budget: &mut usize, action: &mut Option<Action>) {
    if depth >= 9 || *budget == 0 {
        return;
    }
    let Ok(items) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = items
        .take(*budget)
        .filter_map(Result::ok)
        .filter_map(|item| {
            let kind = item.file_type().ok()?;
            let path = item.path();
            let name = filename(&path);
            if name.starts_with('.') || name == "__pycache__" || kind.is_symlink() {
                return None;
            }
            Some((!kind.is_dir(), path))
        })
        .collect();
    items.sort_unstable();
    for (file, path) in items {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let name = filename(&path);
        if file {
            let extension = path.extension().and_then(|text| text.to_str()).unwrap_or_default();
            let is_model = extension == "fieldcraft";
            let is_text = matches!(extension, "py" | "json" | "yaml" | "yml" | "toml" | "csv" | "txt" | "md" | "log" | "lock");
            if ui.add_enabled(is_model || is_text, egui::Button::selectable(active == Some(&path), name)).clicked() {
                *action = Some(if is_model { Action::OpenModel(path) } else { Action::Open(path) });
            }
        } else {
            egui::CollapsingHeader::new(name).id_salt(&path).show(ui, |ui| tree(ui, &path, active, depth + 1, budget, action));
        }
    }
}

fn frame(line: &str) -> Option<(&str, usize)> {
    let (_, at) = line.split_once("File \"")?;
    let (path, at) = at.split_once("\", line ")?;
    let line = at.split(|ch: char| !ch.is_ascii_digit()).next()?.parse().ok()?;
    Some((path, line))
}

fn highlight(ui: &Ui, source: &str, python: bool) -> std::sync::Arc<egui::Galley> {
    let default = ui.visuals().text_color();
    let font = FontId::monospace(13.0);
    let mut job = egui::text::LayoutJob::default();
    let mut segment = |text, color| job.append(text, 0.0, TextFormat::simple(font.clone(), color));
    if python && source.len() <= 64 * 1024 {
        let (keyword, literal, comment) = if ui.visuals().dark_mode {
            (Color32::from_rgb(145, 186, 255), Color32::from_rgb(207, 183, 134), Color32::from_gray(150))
        } else {
            (Color32::from_rgb(36, 76, 158), Color32::from_rgb(132, 70, 32), Color32::from_gray(102))
        };
        for line in source.split_inclusive('\n') {
            let mut indices = line.char_indices().peekable();
            while let Some((start, ch)) = indices.next() {
                let color = match ch {
                    '#' => {
                        segment(&line[start..], comment);
                        break;
                    }
                    '\'' | '"' => {
                        let mut escaped = false;
                        for (_, next) in indices.by_ref() {
                            if next == ch && !escaped {
                                break;
                            }
                            escaped = next == '\\' && !escaped;
                        }
                        literal
                    }
                    ch if ch.is_alphabetic() || ch == '_' => {
                        while indices.next_if(|(_, next)| next.is_alphanumeric() || *next == '_').is_some() {}
                        let end = indices.peek().map_or(line.len(), |(offset, _)| *offset);
                        match &line[start..end] {
                            "and" | "as" | "assert" | "async" | "await" | "break" | "class" | "continue" | "def" | "del" | "elif" | "else"
                            | "except" | "finally" | "for" | "from" | "global" | "if" | "import" | "in" | "is" | "lambda" | "nonlocal" | "not"
                            | "or" | "pass" | "raise" | "return" | "try" | "while" | "with" | "yield" => keyword,
                            "True" | "False" | "None" => literal,
                            _ => default,
                        }
                    }
                    ch if ch.is_ascii_digit() => {
                        while indices.next_if(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_')).is_some() {}
                        literal
                    }
                    _ => default,
                };
                let end = indices.peek().map_or(line.len(), |(offset, _)| *offset);
                segment(&line[start..end], color);
            }
        }
    } else {
        segment(source, default);
    }
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn plot_csv(ui: &mut Ui, text: &str) -> bool {
    let mut rows = text.lines();
    let Some((x_label, headings)) = rows.next().and_then(|line| line.split_once(',')) else { return false };
    let Some(y_label) = headings.split(',').next() else { return false };
    let points: Vec<[f64; 2]> = rows
        .take(4096)
        .filter_map(|line| {
            let mut columns = line.split(',');
            let x: f64 = columns.next()?.trim().parse().ok()?;
            let y: f64 = columns.next()?.trim().parse().ok()?;
            (x.is_finite() && y.is_finite()).then_some([x, y])
        })
        .collect();
    if points.len() < 2 {
        return false;
    }
    let lo = points.iter().fold([f64::INFINITY; 2], |a, b| [a[0].min(b[0]), a[1].min(b[1])]);
    let hi = points.iter().fold([f64::NEG_INFINITY; 2], |a, b| [a[0].max(b[0]), a[1].max(b[1])]);
    if !(hi[0] - lo[0]).is_finite() || !(hi[1] - lo[1]).is_finite() {
        return false;
    }
    let (area, painter) = ui.allocate_painter(vec2(ui.available_width(), 88.0), egui::Sense::hover());
    let rect = area.rect.shrink(8.0);
    let scale = |value: f64, axis: usize| if hi[axis] == lo[axis] { 0.5 } else { ((value - lo[axis]) / (hi[axis] - lo[axis])) as f32 };
    let points = points
        .into_iter()
        .map(|[x, y]| pos2(rect.left() + rect.width() * scale(x, 0), rect.bottom() - rect.height() * scale(y, 1)))
        .collect::<Vec<_>>();
    let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color);
    painter.line_segment([rect.left_top(), rect.left_bottom()], stroke);
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
    let color = ui.visuals().hyperlink_color;
    painter.add(egui::Shape::line(points.clone(), egui::Stroke::new(1.5, color)));
    for point in points {
        painter.circle_filled(point, 2.0, color);
    }
    ui.small(format!("{y_label} vs. {x_label}   [{:.4e}, {:.4e}] × [{:.4e}, {:.4e}]", lo[0], hi[0], lo[1], hi[1]));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_reserves_right_hand_space_and_undocking_restores_it() {
        let ctx = egui::Context::default();
        let mut app = ModelApp::new(fieldcraft_engine::Session::default());
        app.open_python_workspace();
        let mut available = (0.0, 0.0);
        let mut frame = |app: &mut ModelApp| {
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| {
                available.0 = ui.available_width();
                show(app, ui);
                available.1 = ui.available_width();
            });
            output.textures_delta.clear();
            available
        };
        let (full, docked) = frame(&mut app);
        assert!(docked > 550.0 && docked < full - 500.0, "{full} / {docked}");
        app.python_workspace.set_detached(true);
        let (full, detached) = frame(&mut app);
        assert!((full - detached).abs() < 1.0, "{full} / {detached}");
        app.python_workspace.set_detached(false);
        let (_, redocked) = frame(&mut app);
        assert!((redocked - docked).abs() < 1.0, "{docked} / {redocked}");
    }
}
