//! Sorties de l'inventaire.

mod markdown;
mod mermaid;

pub use markdown::markdown;
pub use mermaid::mermaid;

use crate::inventory::Inventory;

pub fn json(inventory: &Inventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}
