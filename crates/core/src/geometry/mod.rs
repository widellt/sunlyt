//! Spacecraft geometry: the OBJ mesh (every triangle occludes), panels found
//! by OBJ object name, and the cell grid laid out on each panel (design D3).

pub mod check;
pub mod layout;
pub mod mesh;
pub mod scene;
pub mod shapes;

pub use layout::{PanelLayout, SceneConfig, StringLayout, WalkOrder};
pub use mesh::Mesh;
pub use scene::{Cell, Panel, Scene, SeriesString};

#[derive(Debug, thiserror::Error)]
pub enum GeometryError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("OBJ: {0}")]
    Obj(String),
    #[error("scene config: {0}")]
    Config(String),
    #[error("OBJ has no object or group named {0:?}")]
    UnknownObject(String),
    #[error("panel {panel:?}: {msg}")]
    Layout { panel: String, msg: String },
}

#[cfg(test)]
mod tests;
