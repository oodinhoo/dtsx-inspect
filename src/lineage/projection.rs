//! Output columns of a SELECT and the table columns they use: enough to follow a
//! column through the query of a source or a lookup.

use std::collections::HashSet;
use std::iter::once;
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, Ident, Query, Select, SelectItem, SetExpr, Statement, TableFactor, TableObject, Visit,
    Visitor,
};

use super::sql::{batches, parse, single_name, table_name, TableName};

/// Output column of a SELECT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectColumn {
    /// Output name: the alias, otherwise the name of the column taken; empty for an
    /// expression without alias, kept for its position (`INSERT INTO t (a, b) SELECT …`).
    pub name: String,
    /// Table columns used: one for a column taken as is, several for an expression.
    pub from: Vec<(TableName, String)>,
    /// Computed (function, operation, conversion…) rather than taken as is.
    pub computed: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Projection {
    pub columns: Vec<SelectColumn>,
    /// Tables of a `SELECT *` or `t.*`: their columns come out under their name.
    pub stars: Vec<TableName>,
}

impl Projection {
    /// Table columns the output column `name` comes from, and whether it is computed.
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

/// List of the first SELECT of the query (the first branch of a union). `None` if it
/// is not a readable SELECT.
// shortcut: a derived table (`FROM (SELECT …) x`) is not traced: its columns stay
// without origin.
pub fn projection(sql: &str) -> Option<Projection> {
    let statements = parse(sql).ok()?;
    let query = statements.iter().find_map(|s| match s {
        Statement::Query(q) => Some(q),
        _ => None,
    })?;
    query_projection(query)
}

/// `INSERT INTO target [(columns)] SELECT …`: the filled table, its named columns
/// (empty without a list) and the SELECT list.
#[derive(Debug)]
pub struct InsertSelect {
    pub target: TableName,
    pub columns: Vec<String>,
    pub projection: Projection,
}

/// The `INSERT … SELECT` of an SQL text, batch by batch (`GO`).
pub fn insert_selects(sql: &str) -> Vec<InsertSelect> {
    batches(sql)
        .iter()
        .filter_map(|batch| parse(batch).ok())
        .flatten()
        .filter_map(|statement| {
            let Statement::Insert(insert) = statement else {
                return None;
            };
            let TableObject::TableName(target) = &insert.table else {
                return None;
            };
            let columns = insert
                .columns
                .iter()
                .filter_map(|c| c.0.last()?.as_ident().map(|id| id.value.clone()))
                .collect();
            Some(InsertSelect {
                target: table_name(target)?,
                columns,
                projection: query_projection(insert.source.as_deref()?)?,
            })
        })
        .collect()
}

fn query_projection(query: &Query) -> Option<Projection> {
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
            // `expr AS (a, b)`: Spark syntax, absent from T-SQL and MySQL.
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
        // Computed column without alias: SQL Server does not name it, nor does SSIS.
        let name = name.unwrap_or_default();
        let mut cited = Identifiers::default();
        // The visitor never breaks (`Break = ()`).
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

/// Tables of the FROM and its joins, with the key designating them in the list: the
/// alias, otherwise the table name (case-insensitive). CTEs are not tables.
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

/// Date parts (`DATEADD(day, …)`): keywords, not columns.
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

/// Columns used in an expression: `name`, `c.name`, `dbo.Customers.name`.
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

    /// `name <- dbo.Table.column, …` for each output column (`=` when computed).
    fn origins(sql: &str) -> Vec<String> {
        let p = projection(sql).expect("readable SELECT");
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
        let sql = "SELECT c.email AS mail, UPPER(p.label) AS country, o.amount
                   FROM dbo.Orders o
                   JOIN dbo.Customers c ON c.id = o.customer_id
                   LEFT JOIN ref.Countries p ON p.code = c.country";
        assert_eq!(
            origins(sql),
            [
                "mail <- dbo.Customers.email",
                "country = ref.Countries.label",
                "amount <- dbo.Orders.amount",
            ]
        );
    }

    #[test]
    fn unqualified_columns_belong_to_the_only_table() {
        assert_eq!(
            origins("SELECT id, DATEADD(day, 1, created_on) AS due FROM dbo.Customers"),
            ["id <- dbo.Customers.id", "due = dbo.Customers.created_on"]
        );
        // Two tables: an unqualified column is ambiguous.
        assert_eq!(origins("SELECT id FROM dbo.A, dbo.B"), ["id <- "]);
    }

    #[test]
    fn stars_keep_the_column_names_of_their_table() {
        let p = projection("SELECT * FROM dbo.Customers").expect("readable SELECT");
        let (from, computed) = p.origin("email").expect("star column");
        assert_eq!(
            (from[0].0.to_string(), from[0].1.as_str(), computed),
            ("dbo.Customers".to_owned(), "email", false)
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
