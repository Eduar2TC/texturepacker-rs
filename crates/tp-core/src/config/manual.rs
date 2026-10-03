use serde::{Deserialize, Serialize};

/// Manual pack-by-folder group: sprites assigned to `name` are packed in
/// their own sheet(s) inside `<output_directory>/<name>/`, separate from the
/// main sheet. The default group (empty `name`) holds every sprite not
/// assigned elsewhere — its sheet stays in the output root. Sprite ids not
/// present in any group go to the default group as well.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FolderGroup {
    /// Sheet name and output subfolder. Empty = main sheet (output root).
    pub name: String,
    /// Sprite ids (normalized paths as in the pipeline) in this group.
    pub sprites: Vec<String>,
}

impl FolderGroup {
    /// Display name of the group in the GUI.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            "(hoja principal)"
        } else {
            &self.name
        }
    }
}

/// Optional snap grid for the Manual algorithm: while dragging in the GUI,
/// positions round to the nearest multiple of `step` on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualGrid {
    /// Grid step in atlas pixels (1..=256).
    pub step: i32,
    /// When true, the free row-flow of the Manual algorithm also starts on
    /// the grid (multiples of `step` for the flow origin and row heights).
    pub snap_flow: bool,
}

impl ManualGrid {
    pub fn new(step: i32, snap_flow: bool) -> Self {
        Self { step, snap_flow }
    }

    /// Round `v` to the nearest multiple of the step (ties away from zero;
    /// negatives clamp up to 0).
    pub fn snap(&self, v: i32) -> i32 {
        let s = self.step.max(1);
        let r = v.rem_euclid(s);
        if r * 2 < s { v - r } else { v + (s - r) }.max(0)
    }

    /// `(x, y)` variant of [`Self::snap`].
    pub fn snap_pos(&self, pos: (i32, i32)) -> (i32, i32) {
        (self.snap(pos.0), self.snap(pos.1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;

    #[test]
    fn manual_grid_snap_rounds_to_multiples() {
        let g = ManualGrid::new(8, true);
        assert_eq!(g.snap(0), 0);
        assert_eq!(g.snap(3), 0);
        assert_eq!(g.snap(4), 8); // empate → arriba
        assert_eq!(g.snap(7), 8);
        assert_eq!(g.snap(16), 16);
        assert_eq!(g.snap(29), 32);
        assert_eq!(g.snap_pos((-3, 10)), (0, 8));
    }

    #[test]
    fn manual_grid_defaults_to_none_and_survives_toml() {
        let cfg = ProjectConfig::default();
        assert!(cfg.manual_grid.is_none());
        let cfg = ProjectConfig {
            manual_grid: Some(ManualGrid::new(16, false)),
            ..ProjectConfig::default()
        };
        let text = cfg.to_toml().unwrap();
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.manual_grid, Some(ManualGrid::new(16, false)));
        // Sin rejilla el TOML no la serializa (proyectos antiguos siguen
        // cargando igual).
        assert!(!ProjectConfig::default()
            .to_toml()
            .unwrap()
            .contains("manual_grid"));
    }
}
