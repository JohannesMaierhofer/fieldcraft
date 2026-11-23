//! Entries from the current mesh-bound external analysis. A result row reuses the
//! same checked field or time-series command as the ribbon and control channel.
use super::{Act, Def, Row, draw_row};
use crate::{ModelApp, results::number};
use fieldcraft_engine::external_catalog::{CatalogField, ExternalCatalog, Quantity, QuantityValue};
use serde_json::{Value, json};

enum Entries<'a> {
    Outputs(&'a [CatalogField], &'static str),
    Integrals(&'a [Quantity]),
    Names(&'a [String]),
}

impl Entries<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Outputs(items, _) => items.len(),
            Self::Integrals(items) => items.len(),
            Self::Names(items) => items.len(),
        }
    }

    fn label(&self, index: usize) -> Option<(&str, String)> {
        match self {
            Self::Outputs(items, _) => items.get(index).map(|item| (item.name.as_str(), item.unit.clone())),
            Self::Integrals(items) => {
                let item = items.get(index)?;
                let value = match &item.value {
                    QuantityValue::Scalar(x) => number(*x),
                    QuantityValue::Vector(x) => format!("[{}]", x.map(number).join(", ")),
                };
                Some((&item.name, format!("{value} {}", item.unit)))
            }
            Self::Names(items) => items.get(index).map(|name| (name.as_str(), String::new())),
        }
    }
}

struct Group<'a> {
    key: &'static str,
    label: &'static str,
    entries: Entries<'a>,
}

fn groups(c: &ExternalCatalog) -> [Group<'_>; 7] {
    [
        Group { key: "external/fields", label: "Fields", entries: Entries::Outputs(&c.fields, "results.external.contour") },
        Group { key: "external/series", label: "Time Series", entries: Entries::Outputs(&c.series, "results.external.series") },
        Group { key: "external/integrals", label: "Integrals", entries: Entries::Integrals(&c.integrals) },
        Group { key: "external/materials", label: "Material Regions", entries: Entries::Names(&c.materials) },
        Group { key: "external/boundaries", label: "Boundaries", entries: Entries::Names(&c.boundaries) },
        Group { key: "external/interfaces", label: "Interfaces", entries: Entries::Names(&c.interfaces) },
        Group { key: "external/loads", label: "Loads", entries: Entries::Names(&c.loads) },
    ]
}

pub(super) fn describe(c: &ExternalCatalog) -> Value {
    let mut out = serde_json::Map::new();
    for group in groups(c) {
        let entries: Vec<_> =
            (0..group.entries.len()).filter_map(|i| group.entries.label(i)).map(|(name, note)| json!({"name": name, "detail": note})).collect();
        if !entries.is_empty() {
            out.insert(group.key.trim_start_matches("external/").to_string(), Value::Array(entries));
        }
    }
    Value::Object(out)
}

pub(super) fn draw(app: &mut ModelApp, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
    let Some(catalog) = app.session.external_catalog().cloned() else { return };
    let expanded = app.tree_open.contains("external");
    let root = draw_row(
        app,
        ui,
        egui::Id::unique("fc_external_analysis"),
        &Row {
            depth: 1,
            fold: Some(expanded),
            icon: Some("results.external.contour"),
            label: "External Analysis",
            node: Some("external"),
            ..Default::default()
        },
    );
    if root.clicked || root.fold {
        acts.push(Act::Fold("external".into()));
    }
    if !expanded {
        return;
    }
    for group in groups(&catalog) {
        let count = group.entries.len();
        if count == 0 {
            continue;
        }
        let open = app.tree_open.contains(group.key);
        let r = draw_row(
            app,
            ui,
            egui::Id::unique(("fc_external_group", group.key)),
            &Row { depth: 2, fold: Some(open), label: group.label, note: Some(count.to_string()), ..Default::default() },
        );
        if let Some(response) = &r.response {
            app.place(group.key, response.rect);
        }
        if r.clicked || r.fold {
            acts.push(Act::Fold(group.key.into()));
        }
        if !open {
            continue;
        }
        for i in 0..count {
            let Some((name, note)) = group.entries.label(i) else { continue };
            let tip = if note.is_empty() { name.to_string() } else { format!("{name} · {note}") };
            let selected = match &group.entries {
                Entries::Outputs(rows, command) => rows.get(i).is_some_and(|row| {
                    app.session.external_selection().is_some_and(|field| field.path == row.path)
                        && (app.session.external_series().is_some() == (*command == "results.external.series"))
                }),
                _ => false,
            };
            let r = draw_row(
                app,
                ui,
                egui::Id::unique((group.key, name)),
                &Row { depth: 3, label: name, name: true, selected, note: Some(note), tip: Some(tip), ..Default::default() },
            );
            if r.clicked {
                match &group.entries {
                    Entries::Outputs(_, command) => acts.push(Act::External(command, name.into())),
                    Entries::Names(_) if group.key != "external/loads" && app.session.model.set(name).is_some() => {
                        acts.push(Act::Select(Def::Set, name.into()));
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_groups_show_values_units_and_only_declared_loads() {
        let mut catalog = ExternalCatalog::default();
        catalog.fields.push(CatalogField { name: "B".into(), unit: "T".into(), path: "field.json".into() });
        catalog.series.push(CatalogField { name: "B".into(), unit: "T".into(), path: "series.json".into() });
        catalog.integrals.push(Quantity { name: "force".into(), unit: "N".into(), value: QuantityValue::Vector([0., 1., 2.]) });
        catalog.materials.push("steel".into());
        catalog.boundaries.push("wall".into());
        catalog.interfaces.push("air gap".into());
        let groups = describe(&catalog);
        assert_eq!(groups["fields"], json!([{"name":"B","detail":"T"}]));
        assert_eq!(groups["series"], groups["fields"]);
        assert_eq!(groups["integrals"][0]["detail"], format!("[0, {}, {}] N", number(1.), number(2.)));
        assert_eq!(groups["materials"][0]["name"], "steel");
        assert_eq!(groups["boundaries"][0]["name"], "wall");
        assert_eq!(groups["interfaces"][0]["name"], "air gap");
        assert!(groups.get("loads").is_none());
        catalog.loads.push("coil excitation".into());
        assert_eq!(describe(&catalog)["loads"][0]["name"], "coil excitation");
    }
}
