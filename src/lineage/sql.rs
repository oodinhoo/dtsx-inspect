//! Tables lues et écrites par une requête T-SQL.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, ObjectName, Query, SelectItem, SetExpr, Statement, TableFactor, TableObject, Visit,
    Visitor,
};
use sqlparser::dialect::{MsSqlDialect, MySqlDialect};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::Token;

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
    pub procedures: Vec<TableName>,
    /// Tables créées (`CREATE TABLE`) : aucune donnée déplacée, mais l'étape
    /// qui prépare la table (souvent temporaire) d'une chaîne de chargement.
    pub creates: Vec<TableName>,
}

/// Tables lues (FROM, JOIN, sous-requêtes), écrites (INSERT, UPDATE, MERGE,
/// SELECT INTO) et créées. TRUNCATE, DELETE et le reste du DDL ne déplacent pas
/// de données. Les lots séparés par `GO` sont lus un par un.
pub fn tables(sql: &str) -> Result<Tables, ParserError> {
    let mut found = Tables::default();
    for batch in batches(sql) {
        match parse(&batch) {
            Ok(statements) => statements.iter().for_each(|s| collect(s, &mut found)),
            // Le parseur refuse certaines options de stockage (`ON [PRIMARY]`…) :
            // pour une création, le nom de la table suffit.
            Err(error) => push_unique(&mut found.creates, created_table(&batch).ok_or(error)?),
        }
    }
    Ok(found)
}

/// Objet défini par un script `.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defined {
    Procedure,
    View,
}

/// Procédures et vues définies (`CREATE [OR ALTER] PROC[EDURE] | VIEW`) et
/// tables lues, écrites ou créées par leur corps.
pub fn definitions(sql: &str) -> Vec<(Defined, TableName, Tables)> {
    batches(sql).iter().filter_map(|b| definition(b)).collect()
}

/// Le T-SQL d'une procédure se passe de `;` et de parenthèses autour des
/// paramètres, et une vue peut porter des options (`WITH SCHEMABINDING`) que le
/// parseur refuse : l'en-tête est lu à la main, puis le corps instruction par
/// instruction.
// ponytail: une instruction refusée est sautée mot par mot jusqu'à la suivante ;
// si l'erreur tombe au milieu d'un INSERT, la sous-requête qui suit compte comme
// lecture et la cible est perdue. Passer à un vrai parseur T-SQL si ça gêne.
fn definition(batch: &str) -> Option<(Defined, TableName, Tables)> {
    let dialect = MsSqlDialect {};
    let mut p = Parser::new(&dialect).try_with_sql(batch).ok()?;
    if !p.parse_keyword(Keyword::CREATE) {
        return None;
    }
    let _ = p.parse_keywords(&[Keyword::OR, Keyword::ALTER]);
    let defined = match p.next_token().token {
        Token::Word(w) if w.keyword == Keyword::VIEW => Defined::View,
        Token::Word(w)
            if w.keyword == Keyword::PROCEDURE || w.value.eq_ignore_ascii_case("proc") =>
        {
            Defined::Procedure
        }
        _ => return None,
    };
    let name = table_name(&p.parse_object_name(false).ok()?)?;
    // Paramètres, colonnes et options jusqu'au `AS` qui ouvre le corps.
    while !p.parse_keyword(Keyword::AS) {
        if p.next_token().token == Token::EOF {
            return None;
        }
    }
    let mut body = Nested(Tables::default());
    loop {
        while p.consume_token(&Token::SemiColon) {}
        if p.peek_token().token == Token::EOF {
            break;
        }
        // Un bloc `BEGIN … END` ou `IF` refusé : on revient à son début et on ne
        // saute que son premier mot, ses instructions sont lues une à une.
        match p.try_parse(|p| p.parse_statement()) {
            // Le visiteur ne s'interrompt jamais (`Break = ()`).
            Ok(statement) => {
                let _ = statement.visit(&mut body);
            }
            Err(_) => {
                p.next_token();
            }
        }
    }
    Some((defined, name, body.0))
}

/// Lit chaque instruction, imbriquée ou non.
struct Nested(Tables);

impl Visitor for Nested {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<()> {
        collect(statement, &mut self.0);
        ControlFlow::Continue(())
    }
}

fn collect(statement: &Statement, found: &mut Tables) {
    if let Statement::Execute {
        name: Some(name), ..
    } = statement
    {
        // ponytail: `EXEC @nom` (procédure dans une variable) est ignoré, comme une table `@t`.
        if let Some(procedure) = table_name(name) {
            push_unique(&mut found.procedures, procedure);
        }
        return;
    }
    if let Statement::CreateTable(create) = statement {
        if let Some(t) = table_name(&create.name) {
            push_unique(&mut found.creates, t);
        }
        return;
    }
    let Some(targets) = write_targets(statement) else {
        return;
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

/// Lots séparés par une ligne `GO` : séparateur de SSMS et de la tâche SQL de
/// SSIS, pas du T-SQL.
fn batches(sql: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::new();
    for line in sql.lines() {
        if line.trim().eq_ignore_ascii_case("go") {
            batches.push(std::mem::take(&mut current));
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    batches.push(current);
    batches.retain(|b| !b.trim().is_empty());
    batches
}

/// `CREATE TABLE nom (…` que le parseur refuse : le nom, lu avant la parenthèse.
fn created_table(batch: &str) -> Option<TableName> {
    const KEYWORD: &str = "create table";
    let batch = batch.trim_start();
    if !batch.get(..KEYWORD.len())?.eq_ignore_ascii_case(KEYWORD) {
        return None;
    }
    object_name(batch[KEYWORD.len()..].split('(').next()?)
}

/// `INSERT … SELECT *` : les colonnes correspondent par leur ordre, fragile.
pub fn inserts_select_star(sql: &str) -> bool {
    any_statement(sql, |s| match s {
        Statement::Insert(insert) => insert.source.as_deref().is_some_and(selects_star),
        _ => false,
    })
}

/// Requête `SELECT *` (source, Lookup) : toutes les colonnes passent dans le flux.
pub fn query_selects_star(sql: &str) -> bool {
    any_statement(sql, |s| matches!(s, Statement::Query(q) if selects_star(q)))
}

fn any_statement(sql: &str, test: impl Fn(&Statement) -> bool) -> bool {
    batches(sql)
        .iter()
        .any(|batch| parse(batch).is_ok_and(|statements| statements.iter().any(&test)))
}

fn selects_star(query: &Query) -> bool {
    match query.body.as_ref() {
        SetExpr::Select(select) => select.projection.iter().any(|item| {
            matches!(
                item,
                SelectItem::Wildcard(..) | SelectItem::QualifiedWildcard(..)
            )
        }),
        _ => false,
    }
}

fn parse(sql: &str) -> Result<Vec<Statement>, ParserError> {
    // Sources ADO.NET/ODBC : la requête peut viser MySQL (MyApp…) plutôt que SQL Server.
    Parser::parse_sql(&MsSqlDialect {}, sql)
        .or_else(|error| Parser::parse_sql(&MySqlDialect {}, sql).map_err(|_| error))
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
pub(crate) fn same(a: &TableName, b: &TableName) -> bool {
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
    fn procedure_bodies_are_read_through_nested_blocks() {
        let sql = "IF EXISTS (SELECT * FROM sys.objects WHERE name = 'P') DROP PROCEDURE [dbo].[P]
                   GO
                   CREATE PROCEDURE [dbo].[P] @mode int AS
                   BEGIN
                   SET NOCOUNT ON;
                   IF @mode = 1
                   BEGIN
                   INSERT INTO dbo.Cible SELECT s.id FROM stg.Source s
                   END
                   EXEC dbo.Autre
                   END
                   GO";
        let found = definitions(sql);

        let names = |v: &[TableName]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(found.len(), 1);
        let (defined, name, t) = &found[0];
        assert_eq!(*defined, Defined::Procedure);
        assert_eq!(name.to_string(), "dbo.P");
        assert_eq!(names(&t.reads), ["stg.Source"]);
        assert_eq!(names(&t.writes), ["dbo.Cible"]);
        assert_eq!(names(&t.procedures), ["dbo.Autre"]);
    }

    #[test]
    fn view_definitions_list_the_tables_they_read() {
        let sql = "CREATE VIEW [dbo].[V] (id, nom) WITH SCHEMABINDING AS
                   WITH c AS (SELECT id FROM dbo.X)
                   SELECT c.id, y.nom AS nom FROM c JOIN dbo.Y y ON y.id = c.id";
        let found = definitions(sql);

        let names = |v: &[TableName]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(found.len(), 1);
        let (defined, name, t) = &found[0];
        assert_eq!(*defined, Defined::View);
        assert_eq!(name.to_string(), "dbo.V");
        assert_eq!(names(&t.reads), ["dbo.X", "dbo.Y"]);
    }

    #[test]
    fn stored_procedures_are_listed() {
        let t = tables("EXEC dbo.ChargerClients @mode = 1").expect("EXEC analysable");
        let names: Vec<String> = t.procedures.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["dbo.ChargerClients"]);
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
    fn go_separated_batches_are_read_one_by_one() {
        let sql =
            "INSERT INTO dbo.A SELECT x FROM stg.A\n  go  \nINSERT INTO dbo.B SELECT y FROM stg.B";
        assert_eq!(
            read_write(sql),
            (strings(&["stg.A", "stg.B"]), strings(&["dbo.A", "dbo.B"]))
        );
    }

    #[test]
    fn created_tables_are_listed_even_with_storage_options() {
        let sql = "IF EXISTS (SELECT * FROM sys.objects WHERE name = 'T') DROP TABLE [dbo].[T]\n\
                   GO\nSET ANSI_NULLS ON\nGO\n\
                   CREATE TABLE [dbo].[T]([id] [int] NULL) ON [PRIMARY]\nGO\n\
                   CREATE TABLE dbo.U (id INT)";
        let t = tables(sql).expect("lots analysables");
        let created: Vec<String> = t.creates.iter().map(ToString::to_string).collect();
        assert_eq!(created, strings(&["dbo.T", "dbo.U"]));
        assert!(t.reads.is_empty() && t.writes.is_empty());
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
