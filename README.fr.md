# dtsx-inspect

[English](README.md) · **Français**

Un analyseur rapide de packages SQL Server Integration Services (SSIS), en ligne de commande,
écrit en Rust. Donnez-lui un fichier `.dtsx` ou tout un dossier de projets SSIS : il dit ce que
contiennent les packages, d'où viennent les données et où elles vont, et quels anti-patterns ils
portent, sans SQL Server, sans Visual Studio et sans connexion à une base.

- **Inventaire** : connexions, tâches et conteneurs, composants de Data Flow, variables,
  paramètres et expressions de propriété.
- **Lignage des tables** : fichiers, tables et entités Dynamics → composants de Data Flow et tâches
  SQL → tables, d'un package à l'autre (sortie Mermaid, ou carte interactive dans le rapport HTML).
- **Lignage par colonne** : d'où vient une colonne et où elle va, à travers les Data Flows et les
  `INSERT … SELECT`, avec une fiche de mapping source → cible pour Excel.
- **Lint** : 11 règles pour les anti-patterns SSIS courants, chacune avec pourquoi c'est un
  problème et comment le corriger.
- **Ordre d'exécution** déduit des données, **graphe des appels de packages**, et **comparaison du
  lignage** entre deux versions d'un dossier (par exemple avant et après une migration de serveur).
- **Rapport HTML** : une page autonome avec recherche, carte du lignage, pages de colonnes, pages
  de packages et constats de lint, en **anglais ou en français** (choix dans la barre latérale).

Les secrets ne sortent pas du package : les mots de passe sont masqués partout, les chaînes de
connexion sont réduites au serveur, à la base et au fournisseur, et le texte SQL n'apparaît que
dans le panneau de détails du rapport HTML, mots de passe masqués.

## Sommaire

- [Compilation](#compilation)
- [Démarrage rapide](#démarrage-rapide)
- [Commandes](#commandes)
- [Recettes](#recettes)
- [Le rapport HTML](#le-rapport-html)
- [Règles de lint](#règles-de-lint)
- [Ce qui est pris en charge](#ce-qui-est-pris-en-charge)
- [Développement](#développement)
- [Licence](#licence)

## Compilation

### Prérequis

- **Rust 1.88 ou plus récent**, installé avec [rustup](https://rustup.rs/).
- **Windows** : l'installateur rustup propose d'installer les *Visual Studio C++ Build Tools*
  (éditeur de liens MSVC et SDK Windows) ; acceptez. Rust est déjà installé ? Mettez-le à jour
  avec `rustup update`.
- **Linux / macOS** : une chaîne de compilation C (`build-essential` sous Debian/Ubuntu, outils
  en ligne de commande Xcode sous macOS).

Vérifier la chaîne :

```sh
rustc --version     # 1.88 ou plus
cargo --version
```

### Compiler

```sh
git clone <url-du-dépôt>
cd dtsx-inspect

cargo build --release
```

L'exécutable est dans `target/release/` :

| Système | Exécutable |
|---|---|
| Windows | `target\release\dtsx-inspect.exe` |
| Linux / macOS | `target/release/dtsx-inspect` |

C'est un fichier unique et autonome (environ 10 Mo) : copiez-le où vous voulez, par exemple sur
un partage réseau ou à côté de vos projets SSIS. Rien d'autre n'est nécessaire pour le lancer.

`cargo build` sans `--release` donne une version de débogage, plus rapide à compiler mais plus
lente, dans `target/debug/`.

### Installer dans le PATH

```sh
cargo install --path .        # copie l'exécutable dans ~/.cargo/bin
dtsx-inspect --version
```

Pour mettre à jour après un `git pull`, relancez la même commande ; `cargo uninstall
dtsx-inspect` le désinstalle.

### Derrière un proxy d'entreprise (Windows)

Si `cargo` n'arrive pas à télécharger les crates à cause d'une erreur de révocation de certificat
(`CRYPT_E_NO_REVOCATION_CHECK`), désactivez la vérification de révocation pour la session :

```powershell
$env:CARGO_HTTP_CHECK_REVOKE = "false"   # PowerShell
cargo build --release
```

```sh
export CARGO_HTTP_CHECK_REVOKE=false     # bash
```

## Démarrage rapide

Sous Windows, **double-cliquez** sur `dtsx-inspect.exe` : il demande un dossier, l'analyse et
ouvre le rapport HTML dans le navigateur. Dans un terminal, le lancer sans argument fait de même.

```sh
dtsx-inspect inventory MonProjet/            # inventaire en Markdown
dtsx-inspect lint MonProjet/                 # anti-patterns, code de sortie 1 en cas d'erreur
dtsx-inspect lineage MonProjet/ --focus dbo.Clients > lignage.mmd
dtsx-inspect --help                          # toutes les commandes
dtsx-inspect lint --help                     # options d'une commande
```

Chaque commande prend un fichier `.dtsx` ou un dossier, parcouru récursivement. Les fichiers
`.conmgr` (connexions de projet) et `.sql` (scripts de projet de base de données) trouvés à côté
des packages sont lus aussi. Les copies de compilation de Visual Studio (`obj/`, `bin/`) sont
ignorées quand les sources sont présentes.

Les sorties de la ligne de commande sont en anglais ; le rapport HTML est disponible en français.

## Commandes

| Commande | Sortie |
|---|---|
| *(aucune)* | Choix d'un dossier, puis rapport HTML dans le navigateur |
| `inventory <chemin> [--format md\|json]` | Connexions, tâches, composants de Data Flow, paramètres, variables, expressions |
| `lineage <chemin> [--focus <texte>]` | `flowchart` Mermaid du lignage des tables ; `--focus` ne garde que ce qui alimente ou lit les tables, fichiers ou entités correspondants |
| `columns <chemin> --of <texte>` | Chaîne amont (←) et aval (→) des colonnes de table ou de fichier correspondantes, par exemple `--of dbo.Clients.email` |
| `mapping <chemin> [--out fichier.csv]` | Mapping source → cible en CSV (`;`, UTF-8 avec BOM, s'ouvre directement dans Excel), une ligne par lien de colonne avec son statut |
| `lint <chemin>` | Une ligne par constat, `gravité[règle] fichier: emplacement: message`, puis les totaux |
| `calls <chemin>` | Graphe Mermaid des tâches Execute Package et des packages qu'elles lancent |
| `order <chemin>` | Ordre d'exécution en Markdown : vagues de packages, cycles, tables partagées écartées, sources à charger d'abord |
| `diff <avant> <après>` | Comparaison en Markdown du lignage par colonne de deux dossiers, serveurs mis à part |

Les recherches (`--focus`, `--of`) ignorent la casse et portent sur une partie du nom : `--of
email` trouve toutes les colonnes dont le nom complet contient `email`.

**Codes de sortie** : `0` si tout va bien, `1` quand un fichier est illisible, qu'une recherche ne
trouve rien, que `lint` trouve une erreur ou que `diff` détecte une régression (lien ou package
disparu, ou lien moins bien suivi). Les erreurs vont sur stderr et les résultats sur stdout : une
redirection ne capture que les résultats.

## Recettes

### Documenter un projet

```sh
dtsx-inspect inventory MonProjet/ > inventaire.md
dtsx-inspect inventory MonProjet/ --format json > inventaire.json
dtsx-inspect order MonProjet/ > ordre-execution.md
```

Le JSON s'interroge facilement, par exemple tous les composants tiers :

```sh
jq '.packages[].executables[].components[]? | select(.known == false) | .class_id' inventaire.json
```

```powershell
(Get-Content inventaire.json -Raw | ConvertFrom-Json).packages.executables.components |
  Where-Object { -not $_.known } | Select-Object name, class_id
```

### Dessiner le lignage en image

Les sorties de `lineage` et `calls` sont des diagrammes [Mermaid](https://mermaid.js.org/).
Collez-les dans un fichier Markdown sur GitHub ou GitLab (dans un bloc ` ```mermaid `), dans
[mermaid.live](https://mermaid.live), ou convertissez-les avec mermaid-cli (Node.js requis) :

```sh
dtsx-inspect lineage MonProjet/ --focus dbo.Clients > clients.mmd
npx -p @mermaid-js/mermaid-cli mmdc -i clients.mmd -o clients.svg

dtsx-inspect calls MonProjet/ > appels.mmd
npx -p @mermaid-js/mermaid-cli mmdc -i appels.mmd -o appels.png
```

Sur un gros dossier, utilisez toujours `--focus` : le lignage complet de centaines de packages est
illisible en image fixe (préférez alors le rapport HTML).

### Suivre une colonne et exporter le mapping

```sh
dtsx-inspect columns MonProjet/ --of dbo.Clients.email
dtsx-inspect mapping MonProjet/ --out mapping.csv
```

```text
Entrepot : dbo.Clients.email (srv-demo)
  ← write: DER Email.email_norm (ChargerClients › DFT Clients)
    ← expression: FF Clients.email (ChargerClients › DFT Clients)
      ← read: D:\demo\clients.csv.email
  → read: SRC Clients.email (Export › DFT Export)
```

Préférez `--out` pour le CSV : une redirection de la console peut le réencoder, et Excel
afficherait alors des accents cassés.

### Contrôler une migration

Analysez le dossier avant et après le changement (nouveau serveur, nouveau fournisseur, packages
retravaillés) :

```sh
dtsx-inspect diff Packages-avant/ Packages-apres/ > comparaison-lignage.md
```

Le compte rendu liste les liens de colonnes disparus, nouveaux et modifiés, les packages disparus
et nouveaux, et les serveurs passés de l'un à l'autre. Le code de sortie vaut `1` si un lien ou un
package a disparu, ou si un lien est moins bien suivi qu'avant.

### Lint en intégration continue

`lint` sort avec le code `1` quand il trouve une erreur (les avertissements et les infos ne font
pas échouer) :

```yaml
# .github/workflows/ssis-lint.yml
name: SSIS lint
on: [push, pull_request]
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install --git <url-du-dépôt> --locked
      - run: dtsx-inspect lint ./ssis
```

Garder la liste des constats à côté du journal de build :

```sh
dtsx-inspect lint MonProjet/ > lint.txt; echo "code de sortie : $?"
```

### Windows PowerShell 5.1 et redirections

Dans Windows PowerShell 5.1, `>` écrit des fichiers en UTF-16, que certains outils ne savent pas
lire. Écrivez explicitement en UTF-8, ou utilisez PowerShell 7 (`pwsh`), dont le `>` écrit en
UTF-8 :

```powershell
dtsx-inspect inventory MonProjet | Out-File -Encoding utf8 inventaire.md
```

## Le rapport HTML

Le rapport est un unique fichier HTML, généré quand le programme est lancé sans argument et écrit
dans le dossier temporaire (`%TEMP%\dtsx-inspect.html` sous Windows). Utilisez
**Enregistrer sous** dans le navigateur pour le garder. Ses espaces :

- **Vue d'ensemble** : chiffres, fiabilité du lignage (part des liens de colonnes directs,
  transformés, par nom et d'origine inconnue), packages les plus signalés, projets.
- **Carte du lignage** : graphe interactif d'un projet ou de tous. Un clic sur un bloc montre sa
  chaîne amont et aval, ses propriétés, sa requête SQL et ses colonnes.
- **Colonnes** : chaque colonne de table et de fichier, avec qui l'écrit et qui la lit, des filtres
  (base, projet, package, sans origine, jamais lue, plusieurs écrivains), une page par colonne et
  une matrice par table. Export du mapping en CSV.
- **Packages** : une page par package avec son flux, son inventaire et ses constats.
- **Qualité** : les constats de lint et une fiche par règle.

Clavier : `Ctrl K` ou `/` pour chercher partout, `F` pour cadrer le graphe, `Échap` pour fermer
le panneau.

La langue suit celle du navigateur (un navigateur en français affiche le français) et se change
avec les boutons **EN / FR** de la barre latérale. Le choix est mémorisé.

La mise en page du graphe ([ELK](https://github.com/kieler/elkjs)), le rendu du Markdown
([marked](https://github.com/markedjs/marked)) et les polices IBM Plex sont chargés depuis des CDN,
épinglés par empreinte (Subresource Integrity). Sans internet, tout fonctionne sauf le graphe et
la mise en forme de l'inventaire.

## Règles de lint

| Règle | Gravité | Signale |
|---|---|---|
| `unresolved-connection` | erreur | Référence de connexion introuvable, ou marquée `:invalid` par SSDT |
| `no-fast-load` | avertissement | Destination OLE DB en insertion ligne à ligne plutôt qu'en chargement rapide |
| `obsolete-provider` | avertissement | Fournisseur OLE DB abandonné (SQLNCLI, SQLOLEDB) |
| `insert-select-star` | avertissement | `INSERT … SELECT *` dans une tâche SQL (colonnes appariées par position) |
| `row-by-row-command` | avertissement | OLE DB Command dans un Data Flow (une requête par ligne) |
| `sort-in-data-flow` | avertissement | Composant Tri, bloquant |
| `unrenamed-package` | avertissement | Package resté nommé `Package` |
| `unused-columns` | avertissement | Source en mode table ou `SELECT *` qui fait passer des colonnes inutiles |
| `default-name` | info | Tâche ou composant resté sous le nom du concepteur (`Data Flow Task 1`…) |
| `disabled-task` | info | Tâche désactivée |
| `incomplete-lineage` | info | Étape que le lignage n'a pas pu suivre (SQL non analysé, requête en variable…) |

Les noms par défaut français de SSDT (`Tâche de flux de données`, `Colonne dérivée`…) sont
reconnus aussi. Le rapport HTML explique chaque règle (pourquoi, comment corriger) dans l'espace
**Qualité**.

## Ce qui est pris en charge

- SSIS 2012 et plus récent (format de package 6 et au-delà), y compris les composants SSIS
  2012/2014 identifiés par GUID.
- Sources, transformations et destinations de Data Flow de Microsoft ; les composants tiers
  (KingswaySoft, composants script…) sont gardés en `Unknown` avec leur identifiant de classe, sans
  jamais planter. Les sources Dynamics 365 de KingswaySoft sont suivies jusqu'à leurs entités,
  FetchXML compris.
- Le SQL des tâches Execute SQL, des sources OLE DB / ADO.NET, des Lookup et des OLE DB Command,
  saisi directement, construit par expression (en pointillés dans le lignage : valeur de
  conception) ou lu dans une variable. Le T-SQL est analysé par
  [sqlparser](https://github.com/apache/datafusion-sqlparser-rs), avec un repli MySQL pour les
  sources ADO.NET.
- Les procédures stockées et les vues définies dans des scripts `.sql` (UTF-8, UTF-8 avec BOM,
  UTF-16) sont développées dans le lignage.

Limites connues : les requêtes lues dans des fichiers ne sont pas suivies, une table dérivée
(`FROM (SELECT …) x`) arrête le lignage par colonne, et `MERGE` / `UPDATE` ne sont suivis qu'au
niveau des tables. La règle `incomplete-lineage` liste chaque étape que le lignage n'a pas pu suivre.

## Développement

```sh
cargo build                                   # compilation de débogage
cargo run -- lint tests/fixtures              # lancer une commande sans installer
cargo run --release -- inventory tests/fixtures/minimal.dtsx

cargo fmt                                     # mise en forme
cargo clippy --all-targets -- -D warnings     # lint, avertissements bloquants
cargo test                                    # tous les tests
cargo test --test lint                        # un fichier de tests (tests/lint.rs)
cargo test focus                              # les tests dont le nom contient « focus »
cargo test -- --nocapture                     # afficher la sortie des tests
```

Les tests d'instantanés utilisent [insta](https://insta.rs). Après un changement voulu d'une
sortie, relisez et acceptez les nouveaux instantanés :

```sh
cargo install cargo-insta    # une fois
cargo insta review
```

Le code est organisé en `parser/` (XML → modèle, sans logique métier), `lineage/` (graphes des
tables et des colonnes, ordre d'exécution), `lint/` (une règle par fichier, trait `Rule`) et
`render/` (Markdown, JSON, Mermaid, CSV, HTML). Tout texte affiché dans le rapport HTML est écrit
en anglais et reçoit sa traduction française dans `src/render/i18n.js`.

Les tests utilisent de petits packages `.dtsx` anonymisés dans `tests/fixtures`. N'ajoutez jamais
de vrais packages de client en fixtures : renommez serveurs, bases, tables et colonnes, et retirez
les chaînes de connexion.

Les contributions sont les bienvenues : ouvrez une issue avec un package anonymisé qui montre le
problème, ou une pull request qui commence par un test en échec. Avant de proposer, `cargo fmt`,
`cargo clippy` et `cargo test` doivent passer.

## Licence

Au choix, sous [licence Apache, version 2.0](LICENSE-APACHE) ou [licence MIT](LICENSE-MIT).
