//! Réduction d'une chaîne de connexion à ce qui peut être affiché.

/// Liste blanche : toute autre clé (mot de passe, jeton, secret…) est ignorée.
const DISPLAYABLE_KEYS: [&str; 5] = [
    "data source",
    "server",
    "initial catalog",
    "database",
    "provider",
];

/// Ne garde que serveur, base et fournisseur. Une chaîne sans `clé=valeur`
/// est un chemin de fichier et reste telle quelle.
pub fn summarize_connection_string(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if !raw.contains('=') {
        return Some(redact_url(raw));
    }
    let kept: Vec<String> = pairs(raw)
        .into_iter()
        .filter(|(key, _)| DISPLAYABLE_KEYS.contains(&key.to_ascii_lowercase().as_str()))
        .map(|(key, value)| format!("{key}={}", redact_url(value)))
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}

/// Retire d'une URL les identifiants (`user:pw@`) et la requête (`?sig=…`).
fn redact_url(value: &str) -> String {
    if !value.contains("://") {
        return value.to_owned();
    }
    let base = value.split(['?', '#']).next().unwrap_or_default();
    let Some(start) = base.find("://").map(|i| i + 3) else {
        return base.to_owned();
    };
    let (scheme, rest) = base.split_at(start);
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let rest = match rest[..authority_end].rfind('@') {
        Some(at) => &rest[at + 1..],
        None => rest,
    };
    format!("{scheme}{rest}")
}

/// Découpe `clé=valeur;…` en respectant les valeurs entre guillemets ou
/// accolades (ODBC), pour qu'un `;` dans un mot de passe ne crée pas une
/// fausse clé affichable.
fn pairs(raw: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut rest = raw;
    loop {
        rest = rest.trim_start_matches(|c: char| c == ';' || c.is_whitespace());
        let Some(eq) = rest.find('=') else {
            return found;
        };
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let (value, tail) = match after.chars().next() {
            Some(open @ ('"' | '\'' | '{')) => match after[1..].find(closing(open)) {
                Some(end) => (&after[1..=end], &after[end + 2..]),
                None => (&after[1..], ""),
            },
            _ => match after.find(';') {
                Some(end) => (after[..end].trim_end(), &after[end..]),
                None => (after.trim_end(), ""),
            },
        };
        found.push((key, value));
        rest = tail;
    }
}

fn closing(open: char) -> char {
    if open == '{' {
        '}'
    } else {
        open
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_connection_string as summarize;

    #[test]
    fn keeps_server_database_and_provider() {
        let raw =
            "Data Source=srv;Initial Catalog=db;Provider=SQLNCLI11.1;Integrated Security=SSPI;";
        assert_eq!(
            summarize(raw).as_deref(),
            Some("Data Source=srv;Initial Catalog=db;Provider=SQLNCLI11.1")
        );
    }

    #[test]
    fn drops_credentials_whatever_their_case() {
        let raw = "server=srv;User ID=u;PWD=x1;password=x2;Database=db";
        assert_eq!(summarize(raw).as_deref(), Some("server=srv;Database=db"));
    }

    #[test]
    fn quoted_password_cannot_leak_into_allowed_keys() {
        let raw = "Password='x;Server=leak';Data Source=srv;PWD=\"y;Database=leak\"";
        assert_eq!(summarize(raw).as_deref(), Some("Data Source=srv"));
    }

    #[test]
    fn unquotes_allowed_values() {
        assert_eq!(
            summarize("Data Source=\"srv;1\"").as_deref(),
            Some("Data Source=srv;1")
        );
    }

    #[test]
    fn keeps_file_path_as_is() {
        assert_eq!(
            summarize(r"D:\demo\entree.csv").as_deref(),
            Some(r"D:\demo\entree.csv")
        );
    }

    #[test]
    fn odbc_braced_password_cannot_leak_into_allowed_keys() {
        let raw = "Driver={SQL Server};Pwd={ab;Server=leak};Database=d";
        assert_eq!(summarize(raw).as_deref(), Some("Database=d"));
    }

    #[test]
    fn strips_credentials_and_query_from_urls() {
        assert_eq!(
            summarize("https://user:pw@host.example/api").as_deref(),
            Some("https://host.example/api")
        );
        assert_eq!(
            summarize("Data Source=https://acct.example/c?sv=1&sig=TOKEN").as_deref(),
            Some("Data Source=https://acct.example/c")
        );
    }

    #[test]
    fn returns_none_when_nothing_is_displayable() {
        assert_eq!(summarize("UserName=u;Password=p"), None);
        assert_eq!(summarize("   "), None);
    }
}
