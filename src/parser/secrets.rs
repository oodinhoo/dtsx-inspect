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

/// Mots qui, dans un nom de variable ou de paramètre, annoncent un secret.
const SECRET_NAMES: [&str; 6] = ["password", "pwd", "motdepasse", "secret", "token", "jeton"];

/// `User::MotDePasse`, `Api_Token`… : la valeur ne doit pas être affichée.
pub fn is_secret_name(name: &str) -> bool {
    let compact = name.to_lowercase().replace(['_', '-', ' '], "");
    SECRET_NAMES.iter().any(|s| compact.contains(s))
}

/// Clés dont la valeur est masquée dans un texte libre (SQL, FetchXML).
const SECRET_KEYS: [&str; 2] = ["password", "pwd"];

/// Masque la valeur de `password=` / `pwd=` (casse ignorée) dans un texte libre :
/// `OPENROWSET(…, 'Server=x;Pwd=secret')`, `CREATE LOGIN … PASSWORD = 'x'`.
// ponytail: repère les clés connues seulement ; un secret sous un autre nom passe.
pub fn redact_secrets(text: &str) -> String {
    // `to_ascii_lowercase` garde les positions d'octets : on cherche dans `lower`
    // et on découpe `text` aux mêmes indices.
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some((at, key)) = SECRET_KEYS
        .iter()
        .filter_map(|k| lower[from..].find(k).map(|i| (from + i, k.len())))
        .min()
    {
        from = at + key;
        let Some(value) = text[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let start = text.len() - value.len();
        let len = match value.chars().next() {
            Some(q @ ('\'' | '"')) => quoted_len(value, q),
            Some('{') => value.find('}').map_or(value.len(), |end| end + 1),
            _ => value
                .find(|c: char| matches!(c, ';' | '\'' | '"' | ')' | ',') || c.is_whitespace())
                .unwrap_or(value.len()),
        };
        out.push_str(&text[copied..start]);
        out.push_str("***");
        copied = start + len;
        from = copied;
    }
    out.push_str(&text[copied..]);
    out
}

/// Longueur d'une valeur entre guillemets, guillemet doublé (`''` en SQL) compris.
fn quoted_len(value: &str, quote: char) -> usize {
    let mut chars = value.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c != quote {
            continue;
        }
        if chars.next_if(|&(_, next)| next == quote).is_none() {
            return i + 1;
        }
    }
    value.len()
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
    let closing = |open: char| if open == '{' { '}' } else { open };
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
    fn masks_passwords_in_free_text() {
        use super::redact_secrets as redact;
        assert_eq!(
            redact("SELECT * FROM OPENROWSET('SQLNCLI', 'Server=s;Uid=u;Pwd=s3cr3t;', 'q')"),
            "SELECT * FROM OPENROWSET('SQLNCLI', 'Server=s;Uid=u;Pwd=***;', 'q')"
        );
        assert_eq!(
            redact("CREATE LOGIN l WITH PASSWORD = 'a b''c', CHECK_POLICY = OFF"),
            "CREATE LOGIN l WITH PASSWORD = ***, CHECK_POLICY = OFF"
        );
        assert_eq!(
            redact("password={x;y} et pwd = \"z\""),
            "password=*** et pwd = ***"
        );
        assert_eq!(
            redact("SELECT PasswordHash FROM t"),
            "SELECT PasswordHash FROM t"
        );
    }

    #[test]
    fn recognises_secret_names() {
        use super::is_secret_name;
        for name in [
            "User::MotDePasse",
            "Mot_de_passe",
            "ApiToken",
            "PWD",
            "$Project::Secret",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["User::Requete", "Chaine", "Passerelle"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn returns_none_when_nothing_is_displayable() {
        assert_eq!(summarize("UserName=u;Password=p"), None);
        assert_eq!(summarize("   "), None);
    }
}
