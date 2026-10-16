//! Colonnes de sortie d'un SELECT et les colonnes de table qu'elles citent : de
//! quoi suivre une colonne à travers la requête d'une source ou d'une recherche.

use std::collections::HashSet;
use std::iter::once;
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, Ident, Select, SelectItem, SetExpr, Statement, TableFactor, Visit, Visitor,
};

use super::sql::{parse, single_name, table_name, TableName};

/// Colonne de sortie d'un SELECT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectColumn {
    /// Nom en sortie : l'alias, sinon le nom de la colonne reprise.
    pub name: String,
    /// Colonnes de table citées : une pour une colonne reprise, plusieurs pour un calcul.
    pub from: Vec<(TableName, String)>,
    /// Calculée (fonction, opération, conversion…) plutôt que reprise telle quelle.
    pub computed: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Projection {
    pub columns: Vec<SelectColumn>,
    /// Tables d'un `SELECT *` ou `t.*` : leurs colonnes sortent sous leur nom.
    pub stars: Vec<TableName>,
}

impl Projection {
    /// Colonnes de table d'où vient la colonne de sortie `name`, et si elle est calculée.
    pub fn origin(&self, name: &str) -> Option<(Vec<(TableName, String)>, bool)> {
        if let Some(c) = self
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
        {
            return Some((c.from.clone(), c.computed));
        }
        match self.stars.as_slice() {
            [table] => Some((vec![(table.clone(), name.to_owned())], false)),
            _ => None,
        }
    }
}

/// Liste du premier SELECT de la requête (la première branche d'une union). `None`
/// si ce n'est pas un SELECT lisible.
// ponytail: une table dérivée (`FROM (SELECT …) x`) n'est pas suivie : ses colonnes
// restent sans origine.
pub fn projection(sql: &str) -> Option<Projection> {
    let statements = parse(sql).ok()?;
    let query = statements.iter().find_map(|s| match s {
        Statement::Query(q) => Some(q),
        _ => None,
    })?;
    let select = first_select(&query.body)?;
    let ctes: HashSet<String> = query
        .with
        .iter()
        .flat_map(|w| &w.cte_tables)
        .map(|c| c.alias.name.value.to_lowercase())
        .collect();
    let tables = from_tables(select, &ctes);
    let resolve = |parts: &[Ident]| -> Option<(TableName, String)> {
        let (column, qualifier) = parts.split_last()?;
        let table = match qualifier {
            [] => match tables.as_slice() {
                [(_, table)] => table,
                _ => return None,
            },
            [.., q] => {
                &tables
                    .iter()
                    .find(|(key, _)| *key == q.value.to_lowercase())?
                    .1
            }
        };
        Some((table.clone(), column.value.clone()))
    };
    let mut projection = Projection::default();
    for item in &select.projection {
        let (expr, alias) = match item {
            SelectItem::UnnamedExpr(expr) => (expr, None),
            SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
            // `expr AS (a, b)` : syntaxe Spark, absente de T-SQL et de MySQL.
            SelectItem::ExprWithAliases { .. } => continue,
            SelectItem::Wildcard(_) => {
                projection
                    .stars
                    .extend(tables.iter().map(|(_, t)| t.clone()));
                continue;
            }
            SelectItem::QualifiedWildcard(kind, _) => {
                let qualifier = kind.to_string().to_lowercase();
                let star = tables.iter().find(|(key, _)| *key == qualifier);
                projection.stars.extend(star.map(|(_, t)| t.clone()));
                continue;
            }
        };
        let name = alias.or_else(|| match expr {
            Expr::Identifier(id) => Some(id.value.clone()),
            Expr::CompoundIdentifier(ids) => ids.last().map(|id| id.value.clone()),
            _ => None,
        });
        // Colonne calculée sans alias : SQL Server ne la nomme pas, SSIS non plus.
        let Some(name) = name else {
            continue;
        };
        let mut cited = Identifiers::default();
        // Le visiteur ne s'interrompt jamais (`Break = ()`).
        let _ = expr.visit(&mut cited);
        let mut from: Vec<(TableName, String)> = Vec::new();
        for column in cited.0.iter().filter_map(|parts| resolve(parts)) {
            if !from.contains(&column) {
                from.push(column);
            }
        }
        projection.columns.push(SelectColumn {
            name,
            from,
            computed: !matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)),
        });
    }
    Some(projection)
}

fn first_select(body: &SetExpr) -> Option<&Select> {
    match body {
        SetExpr::Select(select) => Some(select),
        SetExpr::SetOperation { left, .. } => first_select(left),
        SetExpr::Query(query) => first_select(&query.body),
        _ => None,
    }
}

/// Tables du FROM et de ses jointures, avec la clé qui les désigne dans la liste :
/// l'alias, sinon le nom de la table (sans casse). Les CTE ne sont pas des tables.
fn from_tables(select: &Select, ctes: &HashSet<String>) -> Vec<(String, TableName)> {
    let mut tables = Vec::new();
    for from in &select.from {
        for factor in once(&from.relation).chain(from.joins.iter().map(|j| &j.relation)) {
            let TableFactor::Table { name, alias, .. } = factor else {
                continue;
            };
            if single_name(name).is_some_and(|n| ctes.contains(&n)) {
                continue;
            }
            let Some(table) = table_name(name) else {
                continue;
            };
            let key = alias.as_ref().map_or_else(
                || table.table.to_lowercase(),
                |a| a.name.value.to_lowercase(),
            );
            tables.push((key, table));
        }
    }
    tables
}

/// Parties de date (`DATEADD(day, …)`) : des mots-clés, pas des colonnes.
const DATE_PARTS: [&str; 15] = [
    "year",
    "yyyy",
    "yy",
    "quarter",
    "month",
    "mm",
    "dayofyear",
    "day",
    "dd",
    "week",
    "weekday",
    "hour",
    "minute",
    "second",
    "millisecond",
];

/// Colonnes citées dans une expression : `nom`, `c.nom`, `dbo.Clients.nom`.
#[derive(Default)]
struct Identifiers(Vec<Vec<Ident>>);

impl Visitor for Identifiers {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        match expr {
            Expr::Identifier(id)
                if !id.value.starts_with('@')
                    && !DATE_PARTS.contains(&id.value.to_lowercase().as_str()) =>
            {
                self.0.push(vec![id.clone()]);
            }
            Expr::CompoundIdentifier(ids) => self.0.push(ids.clone()),
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `nom <- dbo.Table.colonne, …` pour chaque colonne de sortie (`=` si calculée).
    fn origins(sql: &str) -> Vec<String> {
        let p = projection(sql).expect("SELECT lisible");
        p.columns
            .iter()
            .map(|c| {
                let from: Vec<String> =
                    c.from.iter().map(|(t, col)| format!("{t}.{col}")).collect();
                let arrow = if c.computed { "=" } else { "<-" };
                format!("{} {arrow} {}", c.name, from.join(", "))
            })
            .collect()
    }

    #[test]
    fn follows_aliases_of_tables_and_columns_through_joins() {
        let sql = "SELECT c.email AS courriel, UPPER(p.libelle) AS pays, o.montant
                   FROM dbo.Commandes o
                   JOIN dbo.Clients c ON c.id = o.client_id
                   LEFT JOIN ref.Pays p ON p.code = c.pays";
        assert_eq!(
            origins(sql),
            [
                "courriel <- dbo.Clients.email",
                "pays = ref.Pays.libelle",
                "montant <- dbo.Commandes.montant",
            ]
        );
    }

    #[test]
    fn unqualified_columns_belong_to_the_only_table() {
        assert_eq!(
            origins("SELECT id, DATEADD(day, 1, cree_le) AS echeance FROM dbo.Clients"),
            ["id <- dbo.Clients.id", "echeance = dbo.Clients.cree_le"]
        );
        // Deux tables : une colonne non qualifiée est ambiguë.
        assert_eq!(origins("SELECT id FROM dbo.A, dbo.B"), ["id <- "]);
    }

    #[test]
    fn stars_keep_the_column_names_of_their_table() {
        let p = projection("SELECT * FROM dbo.Clients").expect("SELECT lisible");
        let (from, computed) = p.origin("email").expect("colonne de l'étoile");
        assert_eq!(
            (from[0].0.to_string(), from[0].1.as_str(), computed),
            ("dbo.Clients".to_owned(), "email", false)
        );
    }

    #[test]
    fn ctes_are_not_tables_and_unions_use_their_first_branch() {
        assert_eq!(
            origins("WITH x AS (SELECT id FROM dbo.A) SELECT x.id FROM x UNION ALL SELECT id FROM dbo.B"),
            ["id <- "]
        );
        assert_eq!(projection("EXEC dbo.P"), None);
    }
}
