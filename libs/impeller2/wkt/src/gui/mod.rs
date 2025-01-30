use crate::Color;
use impeller2::component::Asset;
use impeller2::types::{ComponentId, EntityId};
use nox::{ArrayRepr, Quaternion, Vector3};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub enum Panel {
    Viewport(Viewport),
    VSplit(Split),
    HSplit(Split),
    Graph(Graph),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Split {
    pub panels: Vec<Panel>,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Viewport {
    pub track_entity: Option<EntityId>,
    pub track_rotation: bool,
    pub fov: f32,
    pub active: bool,
    pub pos: Vector3<f32, ArrayRepr>,
    pub rotation: Quaternion<f32, ArrayRepr>,
    pub show_grid: bool,
    pub hdr: bool,
    pub name: Option<String>,
}

impl Viewport {
    pub fn looking_at(mut self, pos: Vector3<f32, ArrayRepr>) -> Self {
        let dir = pos - self.pos;
        let dir = Vector3::new(dir.x(), dir.z(), -dir.y());
        self.rotation = Quaternion::look_at_rh(dir, Vector3::y_axis()).inverse();
        self
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            track_entity: None,
            fov: 45.0,
            active: false,
            pos: Vector3::new(5.0, 5.0, 10.0),
            rotation: Quaternion::identity(),
            track_rotation: true,
            show_grid: false,
            hdr: false,
            name: None,
        }
        .looking_at(Vector3::zeros())
    }
}

impl Asset for Panel {
    const NAME: &'static str = "panel";
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Graph {
    pub entities: Vec<GraphEntity>,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GraphEntity {
    pub entity_id: EntityId,
    pub components: Vec<GraphComponent>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GraphComponent {
    pub component_id: ComponentId,
    pub indexes: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Line3d {
    pub entity: EntityId,
    pub component_id: ComponentId,
    pub index: [usize; 3],
    pub line_width: f32,
    pub color: Color,
    pub perspective: bool,
}

impl Asset for Line3d {
    const NAME: &'static str = "line_3d";
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Camera;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct VectorArrow {
    pub id: ComponentId,
    pub entity_id: EntityId,
    pub range: Range<usize>,
    pub color: Color,
    pub attached: bool,
    pub body_frame: bool,
    pub scale: f32,
}

impl Asset for VectorArrow {
    const NAME: &'static str = "arrow";
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct BodyAxes {
    pub entity_id: EntityId,
    pub scale: f32,
}

impl Asset for BodyAxes {
    const NAME: &'static str = "body_axes";
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub enum Mesh {
    Sphere { radius: f32 },
    Box { x: f32, y: f32, z: f32 },
    Cylinder { radius: f32, height: f32 },
}

impl Mesh {
    pub fn cuboid(x: f32, y: f32, z: f32) -> Self {
        Self::Box { x, y, z }
    }

    pub fn sphere(radius: f32) -> Self {
        Self::Sphere { radius }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Glb(pub String);

impl Asset for Mesh {
    const NAME: &'static str = "mesh";
}

impl Asset for Glb {
    const NAME: &'static str = "glb";
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Component))]
pub struct Material {
    pub base_color: Color,
}

impl Material {
    pub fn color(r: f32, g: f32, b: f32) -> Self {
        Material {
            base_color: Color { r, g, b },
        }
    }
}

impl Asset for Material {
    const NAME: &'static str = "material";
}
