//! Expand data-authored slot ranges into the same clipped/hit-tested widgets.
use super::{
    screen::{Document, Model},
    widgets::{Kind, Widget},
};
use crate::scene::Result;
impl Document {
    pub fn expanded(&self, model: &Model) -> Result<Self> {
        self.validate()?;
        let mut model = model.clone();
        fn expand(mut w: Widget, m: &mut Model) -> Result<Option<Widget>> {
            if w.visible
                .as_ref()
                .is_some_and(|v| v.read(m).is_ok_and(|n| n == 0))
            {
                return Ok(None);
            }
            if let Some(v) = &w.visible {
                v.read(m)?;
            }
            if let Kind::SlotGrid {
                binding,
                columns,
                rows,
                count,
                offset,
                page,
            } = &w.kind
            {
                let columns = columns.read(m)?;
                let rows = rows.read(m)?;
                let count = count.read(m)?;
                let offset = offset.read(m)?;
                if columns == 0 || columns > 16 || rows == 0 || rows > 16 {
                    return Err("Slot grid columns must be 1..16".into());
                }
                let capacity = columns * rows;
                let pages = count.div_ceil(capacity).max(1);
                let raw = page.as_ref().map_or(0., |id| m.number(id));
                if !raw.is_finite() || raw.abs() > 1e6 {
                    return Err("Slot grid page invalid".into());
                }
                let page = (raw.floor() as i64).rem_euclid(pages as i64) as usize;
                let start = page * capacity;
                m.numbers
                    .insert(format!("{}_page", w.id), (page + 1) as f64);
                m.numbers.insert(format!("{}_pages", w.id), pages as f64);
                let cell = [
                    w.layout.size[0] / columns as f32,
                    w.layout.size[1] / rows as f32,
                ];
                for local in 0..capacity.min(count.saturating_sub(start)) {
                    let slot = Widget::new(
                        format!("{}_{}", w.id, start + local),
                        [
                            local as f32 % columns as f32 * cell[0],
                            (local / columns) as f32 * cell[1],
                            cell[0],
                            cell[1],
                        ],
                        Kind::Slot {
                            binding: binding.advanced(offset + start + local)?,
                        },
                    )
                    .styled("slot");
                    w.children.push(slot);
                }
                w.kind = Kind::Panel;
                if w.style.is_empty() {
                    w.style = "grid".into();
                }
            }
            let children = std::mem::take(&mut w.children);
            w.children = children
                .into_iter()
                .map(|c| expand(c, m))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            Ok(Some(w))
        }
        let mut root =
            expand(self.root.clone(), &mut model)?.ok_or("UI root must remain visible")?;
        fn labels(w: &mut Widget, m: &Model) {
            match &mut w.kind {
                Kind::Label { text } | Kind::Button { text, .. } => *text = m.text(text),
                _ => {}
            }
            if let Kind::Button {
                action: super::screen::Action::Command { args, .. },
                ..
            } = &mut w.kind
            {
                for arg in args {
                    *arg = m.text(arg);
                }
            }
            for child in &mut w.children {
                labels(child, m);
            }
        }
        labels(&mut root, &model);
        let doc = Self {
            version: self.version,
            root,
        };
        doc.validate()?;
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ui::framework::{Library, layout::Hit, widgets::Binding};
    use crate::engine_inventory::profile::Profile;
    #[test]
    fn paged_storage_reaches_last_slot_and_uses_shared_hits_and_focus() {
        let p = Profile {
            player_slots: 256,
            hotbar_slots: 16,
            craft_side: 8,
            table_side: 8,
            ..Default::default()
        };
        let mut m = Model::default();
        p.write_model(&mut m);
        m.numbers.insert("storage_page".into(), -1.);
        let library = Library::built_in();
        library.validate_inventory(&p).unwrap();
        let doc = library.resolve("inventory", &m, [420., 300.]).unwrap();
        let rect = doc.binding(Binding::Player(255)).unwrap().0;
        let center = [rect[0] + rect[2] / 2., rect[1] + rect[3] / 2.];
        assert!(matches!(
            doc.hit(center),
            Some(Hit::Slot(Binding::Player(255)))
        ));
        assert!(
            doc.focus_targets()
                .iter()
                .any(|p| (p[0] - center[0]).abs() < 0.001 && (p[1] - center[1]).abs() < 0.001)
        );
        assert!(doc.binding(Binding::Player(16)).is_none());
        assert!(
            doc.widgets
                .iter()
                .any(|w| matches!(&w.widget.kind,Kind::Label{text} if text=="9/9"))
        );
        m.numbers.insert("storage_page".into(), 9.);
        let doc = library.resolve("inventory", &m, [420., 300.]).unwrap();
        assert!(doc.binding(Binding::Player(16)).is_some());
    }
    #[test]
    fn invisible_banks_do_not_hit_and_invalid_ranges_fail_before_expansion() {
        let library = Library::built_in();
        let doc = library
            .resolve("inventory", &Model::default(), [420., 300.])
            .unwrap();
        assert!(
            doc.widgets
                .iter()
                .all(|w| !w.widget.id.starts_with("bank_"))
        );
        let mut value = serde_json::json!({"version":1,"root":{"id":"test","layout":{"size":[100,100]},"kind":"slot_grid","binding":{"source":"player","index":18446744073709551615u64},"columns":1,"rows":1,"count":1}});
        let doc: Document = serde_json::from_value(value.clone()).unwrap();
        assert!(doc.expanded(&Model::default()).is_err());
        value["root"]["binding"]["index"] = serde_json::json!(0);
        value["root"]["columns"] = serde_json::json!(0);
        let doc: Document = serde_json::from_value(value).unwrap();
        assert!(doc.expanded(&Model::default()).is_err());
    }
}
