//! Sorties de l'inventaire.

mod markdown;

pub use markdown::markdown;

use crate::inventory::Inventory;

pub fn json(inventory: &Inventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}
