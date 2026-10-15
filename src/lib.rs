//! dtsx-inspect : analyse de packages SSIS (`.dtsx`).

pub mod calls;
pub mod inventory;
pub mod lineage;
pub mod lint;
pub mod model;
pub mod parser;
pub mod render;

pub use inventory::{inventory, FileError, Inventory};
