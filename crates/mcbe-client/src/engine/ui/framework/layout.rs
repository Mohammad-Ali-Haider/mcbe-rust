//! Anchored layout, bounded resolution and common hit/focus geometry.
use super::{
    screen::Document,
    widgets::{Binding, Kind, Widget},
};
use crate::scene::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default)]
    pub anchor: [Align; 2],
    #[serde(default)]
    pub offset: [f32; 2],
    pub size: [f32; 2],
}
impl Layout {
    pub fn at(r: [f32; 4]) -> Self {
        Self {
            anchor: [Align::Start; 2],
            offset: [r[0], r[1]],
            size: [r[2], r[3]],
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self
            .offset
            .iter()
            .chain(&self.size)
            .any(|f| !f.is_finite() || f.abs() > 16384.)
            || self.size.iter().any(|s| *s <= 0.)
        {
            return Err("Nonfinite, empty or excessive UI layout".into());
        }
        Ok(())
    }
    fn resolve(&self, parent: Rect, scale: f32) -> Rect {
        let mut p = [0.; 2];
        for (a, v) in p.iter_mut().enumerate() {
            let available = parent.0[a + 2] - self.size[a] * scale;
            *v = parent.0[a]
                + self.offset[a] * scale
                + match self.anchor[a] {
                    Align::Start => 0.,
                    Align::Center => available / 2.,
                    Align::End => available,
                };
        }
        Rect([p[0], p[1], self.size[0] * scale, self.size[1] * scale])
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect(pub [f32; 4]);
impl Rect {
    pub fn contains(self, p: [f32; 2]) -> bool {
        p[0] >= self.0[0]
            && p[1] >= self.0[1]
            && p[0] < self.0[0] + self.0[2]
            && p[1] < self.0[1] + self.0[3]
    }
    pub fn intersect(self, other: Self) -> Self {
        let x = self.0[0].max(other.0[0]);
        let y = self.0[1].max(other.0[1]);
        Rect([
            x,
            y,
            (self.0[0] + self.0[2]).min(other.0[0] + other.0[2]).max(x) - x,
            (self.0[1] + self.0[3]).min(other.0[1] + other.0[3]).max(y) - y,
        ])
    }
}
pub struct ResolvedWidget {
    pub widget: Widget,
    pub rect: Rect,
    pub clip: Rect,
}
pub struct ResolvedScreen {
    pub widgets: Vec<ResolvedWidget>,
    pub scale: f32,
}
#[derive(Clone)]
pub enum Hit {
    Slot(Binding),
    Search,
    Catalog(usize),
    Button(usize),
}
impl Document {
    pub fn resolve(&self, viewport: [f32; 2]) -> Result<ResolvedScreen> {
        self.validate()?;
        if viewport.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err("UI viewport invalid".into());
        }
        let scale = ((viewport[0] - 8.).max(1.) / self.root.layout.size[0])
            .min((viewport[1] - 8.).max(1.) / self.root.layout.size[1])
            .min(1.);
        let viewport = Rect([0., 0., viewport[0], viewport[1]]);
        let mut result = ResolvedScreen {
            widgets: Vec::new(),
            scale,
        };
        fn visit(
            widget: &Widget,
            parent: Rect,
            clip: Rect,
            scale: f32,
            result: &mut Vec<ResolvedWidget>,
        ) {
            let rect = widget.layout.resolve(parent, scale);
            let clip = clip.intersect(rect);
            result.push(ResolvedWidget {
                widget: widget.clone(),
                rect,
                clip,
            });
            for child in &widget.children {
                visit(child, rect, clip, scale, result);
            }
        }
        visit(&self.root, viewport, viewport, scale, &mut result.widgets);
        Ok(result)
    }
}
impl ResolvedScreen {
    pub fn binding(&self, binding: Binding) -> Option<Rect> {
        self.widgets.iter().find_map(|w| {
            matches!(w.widget.kind,Kind::Slot{binding:b} if b==binding).then_some(w.rect)
        })
    }
    pub fn preview(&self) -> Option<Rect> {
        self.widgets
            .iter()
            .find_map(|w| matches!(w.widget.kind, Kind::Preview).then_some(w.rect))
    }
    pub fn catalog(&self) -> Option<(&ResolvedWidget, usize, usize)> {
        self.widgets.iter().find_map(|w| {
            if let Kind::Catalog { columns, rows } = w.widget.kind {
                Some((w, columns, rows))
            } else {
                None
            }
        })
    }
    pub fn hit(&self, p: [f32; 2]) -> Option<Hit> {
        self.widgets
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, w)| {
                if !w.rect.contains(p) || !w.clip.contains(p) {
                    return None;
                }
                match &w.widget.kind {
                    Kind::Slot { binding } => Some(Hit::Slot(*binding)),
                    Kind::Search => Some(Hit::Search),
                    Kind::Button { .. } => Some(Hit::Button(index)),
                    Kind::Catalog { columns, rows } => {
                        let x = ((p[0] - w.rect.0[0]) / w.rect.0[2] * (*columns as f32)).floor()
                            as usize;
                        let y =
                            ((p[1] - w.rect.0[1]) / w.rect.0[3] * (*rows as f32)).floor() as usize;
                        Some(Hit::Catalog(y * columns + x))
                    }
                    _ => None,
                }
            })
    }
    pub fn focus_targets(&self) -> Vec<[f32; 2]> {
        let mut points = Vec::new();
        for w in &self.widgets {
            if matches!(
                w.widget.kind,
                Kind::Slot { .. } | Kind::Button { .. } | Kind::Search
            ) {
                points.push([
                    w.rect.0[0] + w.rect.0[2] / 2.,
                    w.rect.0[1] + w.rect.0[3] / 2.,
                ]);
            }
        }
        points
    }
    pub fn neighbor(&self, p: [f32; 2], direction: [f32; 2]) -> Option<[f32; 2]> {
        self.focus_targets()
            .into_iter()
            .filter(|v| (v[0] - p[0]) * direction[0] + (v[1] - p[1]) * direction[1] > 0.01)
            .min_by(|a, b| {
                let score = |v: [f32; 2]| {
                    let x = v[0] - p[0];
                    let y = v[1] - p[1];
                    x * x + y * y + 4. * (x * direction[1] - y * direction[0]).powi(2)
                };
                score(*a).total_cmp(&score(*b))
            })
    }
}
