//! Tables lues et écrites par une requête T-SQL.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, ObjectName, Query, SelectItem, SetExpr, Statement, TableFactor, TableObject, Visit,
    Visitor,
};
use sqlparser::dialect::{MsSqlDialect, MySqlDialect};
use sqlparser::parser::{Parser, ParserError};

/// Nom de table tel qu'écrit (sans crochets ni guillemets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableName {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
}

impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in [&self.database, &self.schema].into_iter().flatten() {
            write!(f, "{part}.")?;
        }
        write!(f, "{}", self.table)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tables {
    pub reads: Vec<TableName>,
    pub writes: Vec<TableName>,
    /// Procédures stockées appelées (`EXEC`) : leur contenu n'est pas visible.
    pub procedures: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("SQL non analysable : {0}")]
pub struct SqlError(#[from] ParserError);

/// Tables lues (FROM, JOIN, sous-requêtes) et écrites (INSERT, UPDATE, MERGE,
/// SELECT INTO). Le DDL, TRUNCATE et DELETE ne déplacent pas de données.
pub fn tables(sql: &str) -> Result<Tables, SqlError> {
    let statements = parse(sql)?;
    let mut found = Tables::default();
    for statement in statements {
        if let Statement::Execute {
            name: Some(name), ..
        } = &statement
        {
            let procedure = table_name(name).map_or_else(|| name.to_string(), |p| p.to_string());
            found.procedures.push(procedure);
            continue;
        }
        let Some(targets) = write_targets(&statement) else {
            continue;
        };
        let mut seen = Relations::default();
        // Le visiteur ne s'interrompt jamais (`Break = ()`).
        let _ = statement.visit(&mut seen);
        let writes: Vec<TableName> = targets
            .iter()
            .filter_map(|t| table_name(seen.unalias(t)))
            .collect();
        let reads = seen
            .relations
            .iter()
            .filter(|r| !seen.is_cte(r) && !seen.is_alias(r))
            .filter_map(table_name)
            .filter(|t| !writes.iter().any(|w| same(w, t)));
        for t in reads {
            push_unique(&mut found.reads, t);
        }
        for t in writes {
            push_unique(&mut found.writes, t);
        }
    }
    Ok(found)
}

/// `INSERT … SELECT *` : les colonnes correspondent par leur ordre, fragile.
pub fn inserts_select_star(sql: &str) -> bool {
    let star = |query: &Query| match query.body.as_ref() {
        SetExpr::Select(select) => select.projection.iter().any(|item| {
            matches!(
                item,
                SelectItem::Wildcard(..) | SelectItem::QualifiedWildcard(..)
            )
        }),
        _ => false,
    };
    parse(sql).is_ok_and(|statements| {
        statements.iter().any(|s| match s {
            Statement::Insert(insert) => insert.source.as_deref().is_some_and(star),
            _ => false,
        })
    })
}

fn parse(sql: &str) -> Result<Vec<Statement>, SqlError> {
    // Sources ADO.NET/ODBC : la requête peut viser MySQL (MyApp…) plutôt que SQL Server.
    Ok(Parser::parse_sql(&MsSqlDialect {}, sql)
        .or_else(|error| Parser::parse_sql(&MySqlDialect {}, sql).map_err(|_| error))?)
}

/// Nom d'objet seul, par exemple la propriété `OpenRowset` (`[dbo].[Table]`).
pub fn object_name(name: &str) -> Option<TableName> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut closing = None;
    for c in name.trim().chars() {
        match (closing, c) {
            (Some(close), c) if c == close => closing = None,
            (Some(_), c) => current.push(c),
            (None, '[') => closing = Some(']'),
            (None, '"') => closing = Some('"'),
            (None, '.') => parts.push(std::mem::take(&mut current)),
            (None, c) => current.push(c),
        }
    }
    parts.push(current);
    from_parts(parts)
}

/// Cibles d'écriture ; `None` = instruction sans déplacement de données.
fn write_targets(statement: &Statement) -> Option<Vec<ObjectName>> {
    let table = |factor: &TableFactor| match factor {
        TableFactor::Table { name, .. } => Some(name.clone()),
        _ => None,
    };
    match statement {
        Statement::Insert(insert) => Some(match &insert.table {
            TableObject::TableName(name) => vec![name.clone()],
            _ => Vec::new(),
        }),
        Statement::Update(update) => Some(table(&update.table.relation).into_iter().collect()),
        Statement::Merge(merge) => Some(table(&merge.table).into_iter().collect()),
        Statement::Query(query) => Some(select_into(query)),
        _ => None,
    }
}

fn select_into(query: &Query) -> Vec<ObjectName> {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Vec::new();
    };
    select
        .into
        .iter()
        .flat_map(|into| &into.targets)
        .filter_map(|target| match target {
            Expr::Identifier(id) => Some(ObjectName::from(id.clone())),
            Expr::CompoundIdentifier(ids) => Some(ObjectName::from(ids.clone())),
            _ => None,
        })
        .collect()
}

/// Tables citées dans l'instruction, noms de CTE et alias (qui n'en sont pas).
// ponytail: CTE et alias collectés pour toute l'instruction, pas par portée ;
// un alias ou une CTE homonyme d'une vraie table dans une autre sous-requête la masque.
#[derive(Default)]
struct Relations {
    relations: Vec<ObjectName>,
    ctes: HashSet<String>,
    /// Alias (minuscules) -> table : `FROM dbo.Clients c`.
    aliases: HashMap<String, ObjectName>,
}

impl Relations {
    fn is_cte(&self, relation: &ObjectName) -> bool {
        single_name(relation).is_some_and(|name| self.ctes.contains(&name))
    }

    /// Alias vers une autre table (`FROM Clients clients` reste une table).
    fn is_alias(&self, relation: &ObjectName) -> bool {
        single_name(relation)
            .and_then(|name| self.aliases.get(&name))
            .is_some_and(|target| {
                !target
                    .to_string()
                    .eq_ignore_ascii_case(&relation.to_string())
            })
    }

    /// `UPDATE c … FROM dbo.Clients c` : la cible `c` est `dbo.Clients`.
    fn unalias<'a>(&'a self, relation: &'a ObjectName) -> &'a ObjectName {
        single_name(relation)
            .and_then(|name| self.aliases.get(&name))
            .unwrap_or(relation)
    }
}

fn single_name(relation: &ObjectName) -> Option<String> {
    match relation.0.as_slice() {
        [single] => single.as_ident().map(|id| id.value.to_lowercase()),
        _ => None,
    }
}

impl Visitor for Relations {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if let Some(with) = &query.with {
            let names = with
                .cte_tables
                .iter()
                .map(|c| c.alias.name.value.to_lowercase());
            self.ctes.extend(names);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        self.relations.push(relation.clone());
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table {
            name,
            alias: Some(alias),
            ..
        } = factor
        {
            self.aliases
                .insert(alias.name.value.to_lowercase(), name.clone());
        }
        ControlFlow::Continue(())
    }
}

fn table_name(name: &ObjectName) -> Option<TableName> {
    let parts = name
        .0
        .iter()
        .filter_map(|p| p.as_ident())
        .map(|id| id.value.clone())
        .collect();
    from_parts(parts)
}

/// `[serveur.]base.schéma.table` ; tables temporaires et variables ignorées.
fn from_parts(mut parts: Vec<String>) -> Option<TableName> {
    let mut next = || {
        parts
            .pop()
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
    };
    let table = next()?;
    if table.starts_with(['#', '@']) {
        return None;
    }
    let schema = next();
    let database = next();
    Some(TableName {
        database,
        schema,
        table,
    })
}

/// Même table pour SQL Server : casse ignorée (collation par défaut), schéma
/// `dbo` par défaut, base comparée seulement si les deux la précisent.
fn same(a: &TableName, b: &TableName) -> bool {
    let schema = |t: &TableName| t.schema.as_deref().unwrap_or("dbo").to_lowercase();
    let same_database = match (&a.database, &b.database) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        _ => true,
    };
    same_database && schema(a) == schema(b) && a.table.eq_ignore_ascii_case(&b.table)
}

fn push_unique(list: &mut Vec<TableName>, table: TableName) {
    if !list.iter().any(|t| same(t, &table)) {
        list.push(table);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_write(sql: &str) -> (Vec<String>, Vec<String>) {
        let t = tables(sql).expect("SQL analysable");
        let names = |v: &[TableName]| v.iter().map(ToString::to_string).collect();
        (names(&t.reads), names(&t.writes))
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn insert_select_reads_sources_and_writes_target() {
        let sql = "INSERT INTO dbo.Clients (id) SELECT c.id FROM stg.Clients AS c \
                   JOIN ref.Pays p ON p.code = c.pays";
        assert_eq!(
            read_write(sql),
            (
                strings(&["stg.Clients", "ref.Pays"]),
                strings(&["dbo.Clients"])
            )
        );
    }

    #[test]
    fn select_only_reads() {
        assert_eq!(
            read_write("select * from [ref].[Pays]"),
            (strings(&["ref.Pays"]), vec![])
        );
    }

    #[test]
    fn three_part_names_keep_their_database() {
        assert_eq!(
            read_write("SELECT id FROM Entrepot.dbo.Clients"),
            (strings(&["Entrepot.dbo.Clients"]), vec![])
        );
    }

    #[test]
    fn update_from_join_writes_target_and_reads_the_others() {
        let sql = "UPDATE dbo.Clients SET maj = 1 FROM dbo.Clients c \
                   JOIN stg.Delta d ON d.id = c.id";
        assert_eq!(
            read_write(sql),
            (strings(&["stg.Delta"]), strings(&["dbo.Clients"]))
        );
    }

    #[test]
    fn merge_reads_source_and_writes_target() {
        let sql = "MERGE dbo.Clients AS t USING stg.Clients AS s ON t.id = s.id \
                   WHEN MATCHED THEN UPDATE SET t.nom = s.nom \
                   WHEN NOT MATCHED THEN INSERT (id, nom) VALUES (s.id, s.nom);";
        assert_eq!(
            read_write(sql),
            (strings(&["stg.Clients"]), strings(&["dbo.Clients"]))
        );
    }

    #[test]
    fn select_into_writes_target() {
        assert_eq!(
            read_write("SELECT * INTO dbo.Copie FROM dbo.Clients"),
            (strings(&["dbo.Clients"]), strings(&["dbo.Copie"]))
        );
    }

    #[test]
    fn cte_names_are_not_tables() {
        let sql = "WITH recent AS (SELECT * FROM dbo.Ventes) \
                   SELECT * FROM recent JOIN dbo.Clients c ON c.id = recent.client";
        assert_eq!(
            read_write(sql),
            (strings(&["dbo.Ventes", "dbo.Clients"]), vec![])
        );
    }

    #[test]
    fn insert_values_only_writes() {
        assert_eq!(
            read_write("INSERT INTO dbo.Log (msg) VALUES ('x')"),
            (vec![], strings(&["dbo.Log"]))
        );
    }

    #[test]
    fn ddl_truncate_and_delete_move_no_data() {
        for sql in [
            "TRUNCATE TABLE stg.Clients",
            "DROP TABLE dbo.Temp",
            "DELETE FROM dbo.Clients WHERE id IN (SELECT id FROM stg.Purge)",
        ] {
            assert_eq!(read_write(sql), (vec![], vec![]), "{sql}");
        }
    }

    #[test]
    fn several_statements_are_combined() {
        assert_eq!(
            read_write("DELETE FROM dbo.T; INSERT INTO dbo.T SELECT * FROM stg.T"),
            (strings(&["stg.T"]), strings(&["dbo.T"]))
        );
    }

    #[test]
    fn temp_tables_and_table_variables_are_ignored() {
        assert_eq!(
            read_write("INSERT INTO #tmp SELECT * FROM dbo.T"),
            (strings(&["dbo.T"]), vec![])
        );
    }

    #[test]
    fn update_through_an_alias_writes_the_real_table() {
        let sql = "UPDATE c SET maj = 1 FROM dbo.Clients c JOIN stg.Delta d ON d.id = c.id";
        assert_eq!(
            read_write(sql),
            (strings(&["stg.Delta"]), strings(&["dbo.Clients"]))
        );
    }

    #[test]
    fn a_table_without_schema_is_the_dbo_table() {
        let sql = "UPDATE T SET x = 1 FROM dbo.T JOIN stg.U u ON u.id = T.id";
        assert_eq!(read_write(sql), (strings(&["stg.U"]), strings(&["T"])));
    }

    #[test]
    fn stored_procedures_are_listed() {
        let t = tables("EXEC dbo.ChargerClients @mode = 1").expect("EXEC analysable");
        assert_eq!(t.procedures, vec!["dbo.ChargerClients"]);
        assert!(t.reads.is_empty() && t.writes.is_empty());
    }

    #[test]
    fn mysql_source_queries_are_understood_too() {
        // Sources ADO.NET vers MySQL (MyApp) : CONVERT(expr, type) n'est pas du T-SQL.
        let sql =
            "SELECT CONVERT(U.id, SIGNED INTEGER) AS USER_ID, COALESCE(U.realname, \"\") AS NOM \
                   FROM app_users U LEFT OUTER JOIN app_profiles P ON U.profiles_id = P.id";
        assert_eq!(
            read_write(sql),
            (strings(&["app_users", "app_profiles"]), vec![])
        );
    }

    #[test]
    fn spots_insert_select_star() {
        assert!(inserts_select_star("INSERT INTO a SELECT * FROM b"));
        assert!(inserts_select_star(
            "INSERT INTO a (x, y) SELECT t.* FROM b t"
        ));
        assert!(!inserts_select_star("INSERT INTO a SELECT x FROM b"));
        assert!(!inserts_select_star("SELECT * FROM b"));
        assert!(!inserts_select_star("BULK INSERT a FROM 'f'"));
    }

    #[test]
    fn unparsable_sql_is_an_error() {
        assert!(tables("BULK INSERT dbo.Import FROM 'D:\\demo\\import.csv'").is_err());
    }

    #[test]
    fn object_name_strips_brackets() {
        let name = object_name("[stg].[Clients]").map(|n| n.to_string());
        assert_eq!(name.as_deref(), Some("stg.Clients"));
        let sheet = object_name("Feuil1$").map(|n| n.to_string());
        assert_eq!(sheet.as_deref(), Some("Feuil1$"));
        assert_eq!(object_name("  "), None);
    }
}
