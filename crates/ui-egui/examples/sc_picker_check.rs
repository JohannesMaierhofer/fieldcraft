//! Fork verification helper: open a local native document through both GUI picker configurations.
use fieldcraft_engine::Session;
use fieldcraft_ui_egui::{FilePicker, ModelApp, Services, files::Kind};
use serde_json::json;
use std::time::{Duration, Instant};

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: sc_picker_check FILE.sc")?;
    for kind in [None, Some(Kind::Design)] {
        let selected = path.clone();
        let services = Services {
            pick_file: Some(Box::new(move |request| {
                let path = match request {
                    FilePicker::Open { filters } if filters.first().is_some_and(|f| f.extensions.contains(&"sc")) => Some(selected.clone()),
                    _ => None,
                };
                Box::pin(std::future::ready(path))
            })),
            ..Default::default()
        };
        let mut app = ModelApp::new(Session::default()).with_services(services);
        let ctx = egui::Context::default();
        app.open_dialog(kind);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            app.logic(&ctx);
            if app.session.design.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("{kind:?} did not open {path}"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let bodies = app.run("geometry.bodies", json!({}))?;
        println!("{}", json!({"picker":format!("{kind:?}"),"bodies":bodies}));
    }
    Ok(())
}
