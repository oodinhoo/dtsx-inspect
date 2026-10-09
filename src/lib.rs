//! dtsx-inspect : analyse de packages SSIS (`.dtsx`).

pub mod inventory;
pub mod model;
pub mod parser;
pub mod render;

pub use inventory::{inventory, FileError, Inventory};
