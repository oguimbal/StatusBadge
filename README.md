# Status Badge

Overlay macOS qui affiche l’état de plusieurs interfaces d’administration (« sudo »). L’application ne connaît aucun projet : chaque sudo est ajouté par son URL et implémente le [protocole Status Badge v1](docs/protocol.md).

Rust + GPUI, au même commit qu’AgentsMon (`3767d69d6470f028411cfb75d25adbe5548ff9ca`), avec `runtime_shaders`, `font-kit` et AppKit via `objc2`.

## Utilisation

- Badge carré de 64 points, sans icône Dock, au-dessus des fenêtres et sur tous les Spaces. Glisser pour déplacer ; la position est sauvegardée.
- **Centre** (24 points) : statut combiné des sudos de **production**, avec la somme de leurs alertes. Un sudo verrouillé ou dont les données sont périmées compte comme inconnu. Sans aucun sudo de production, le centre combine tous les sudos. Le staging et le développement ne colorent que leur propre satellite.
- **Bulbes** : chaque sudo de production est un bulbe de 18 points dessiné derrière le centre, dont seule la partie extérieure dépasse, avec son nombre d’alertes. Les bulbes se répartissent régulièrement autour du centre.
- **Oreilles** : un staging ou un développement est une oreille de 10 points dessinée derrière le bulbe de production du même nom de projet, sans compteur. Sans production du même nom, il devient un bulbe plus petit (14 points). Toutes les formes rétrécissent ensemble quand les sudos sont trop nombreux pour que deux projets ne se touchent pas ; au-delà de huit projets, elles peuvent se toucher.
- L’environnement est déclaré par le sudo lors de la connexion, pas choisi dans le badge.
- Badge vide : centre gris avec « + ». Clic : saisie de l’URL du sudo.
- Clic sur un satellite : URL `open` déclarée par le sudo (en général une route qui choisit la rubrique selon le statut). Cadenas : reconnexion dans le navigateur.
- Clic sur le centre : sudo de production le plus grave.
- Survol du centre : liste des sudos. Survol d’un satellite : rubriques et compteurs fournis par son résumé.
- Clic droit : ajouter un sudo, retirer un sudo, mise à jour, replacer, quitter.
- Rouge : pulsation du cercle et de son halo toutes les 2,8 secondes, sans mouvement. Réduire les animations : rouge et halo fixes.

Chaque sudo est interrogé toutes les 3 secondes après la réponse précédente. Le statut global et le compteur viennent du serveur ; l’application ne les recalcule pas. Une erreur réseau, `429`, `5xx` ou réponse invalide conserve la session et signale des données périmées. `401`/`403`, ou l’expiration déclarée par le sudo, efface le jeton et affiche le cadenas. Au-delà de 30 secondes sans données, un statut vert devient inconnu ; les alertes déjà reçues restent visibles.

## Ajouter un sudo

1. Clic droit → **Ajouter un sudo…** (ou clic sur le badge vide), puis saisir l’URL de son interface d’administration, par exemple `https://sudo.example.com`. HTTPS obligatoire ; HTTP seulement pour `localhost` et `*.localhost`.
2. Le navigateur ouvre `<url>/statusbadge?state=…`. Se connecter si nécessaire.
3. Autoriser le navigateur à ouvrir Status Badge. Le sudo renvoie `statusbadge://connect#…` avec un jeton de lecture du résumé.
4. Le badge vérifie la demande (nonce de 256 bits, 10 minutes, usage unique), interroge une première fois le résumé, puis enregistre le jeton dans le Trousseau.

Une connexion refusée n’enregistre rien et repropose la saisie. Reconnecter une URL déjà enregistrée remplace son jeton et ses métadonnées.

Installer le `.app` pour enregistrer `statusbadge://`. `cargo run` permet le rendu mais ne remplace pas l’enregistrement Launch Services du bundle. Une demande de connexion commencée avant de quitter l’application doit être recommencée.

## Sécurité

- Jetons uniquement dans le Trousseau : service `fr.gmbl.statusbadge.session`, un compte par sudo (identifiant local aléatoire). Préférences JSON (`~/Library/Application Support/fr.gmbl.StatusBadge/config.json`) sans secret.
- Aucun jeton ni lien de retour dans les logs. Les requêtes authentifiées ne suivent pas les redirections et n’envoient pas d’en-tête `Origin`.
- Le jeton n’est enregistré qu’après une première réponse valide du résumé.
- Le domaine de `summary` n’est pas contraint (un résumé peut être servi par un autre domaine que l’interface d’administration) ; la protection repose sur HTTPS et sur la page sudo authentifiée qui produit le lien.

## Développement

macOS 13+, Rust stable via rustup, outils de développement Apple. Python 3.11+ pour les scripts de version et leurs tests. `crates/statusbadge-core` : contrat, protocole et validations ; `crates/statusbadge-ui` : vues et géométrie ; `crates/statusbadge` : AppKit, HTTP, Trousseau.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo run -p statusbadge --locked -- --demo
./bundle/bundle.sh
open 'dist/Status Badge.app'
```

`--demo` affiche sept sudos fictifs (`fixtures/monitoring-summary.json`) et une mise à jour fictive ; il ne lit ni ne modifie le Trousseau et n’appelle aucune API. `BUNDLE_PROFILE=debug CODESIGN_IDENTITY=- ./bundle/bundle.sh` prépare un bundle local de test.

La saisie d’URL passe par une boîte de dialogue AppleScript (`osascript`) dans un thread : GPUI n’a pas de champ texte à cette révision, et une fenêtre modale AppKit sur le thread principal réentrerait dans la boucle GPUI.

## Téléchargement et mises à jour

DMG signé et notarisé (macOS 13+, Apple Silicon et Intel) : [`StatusBadge.dmg`](https://github.com/oguimbal/StatusBadge/releases/latest/download/StatusBadge.dmg), attaché à la [dernière release](https://github.com/oguimbal/StatusBadge/releases/latest).

Status Badge interroge l’API GitHub (`releases/latest` de ce dépôt) au lancement, puis toutes les 24 heures, et 30 minutes après un échec. La requête est anonyme et porte la version installée dans son `User-Agent`. Quand une version supérieure est publiée avec son DMG, une flèche apparaît en haut à droite du badge :

1. Clic sur la flèche : le DMG est téléchargé dans Téléchargements, sans remplacer de fichier existant. Sa taille et son empreinte SHA-256 sont comparées à celles publiées par GitHub, puis Gatekeeper vérifie la signature Developer ID et la notarisation.
2. Le DMG s’ouvre dans le Finder : glisser Status Badge sur Applications et accepter le remplacement.
3. Le badge détecte en moins d’une minute que le bundle installé porte une version supérieure, et redémarre dessus.

Clic droit → **Rechercher une mise à jour** lance la vérification immédiatement ; sa réponse (« ✓ » à jour, « ! » vérification impossible) reste quelques secondes sur le badge. Un téléchargement refusé (taille, empreinte ou Gatekeeper) affiche « ! » ; un clic réessaie. Il n’y a pas de réglage pour désactiver la vérification. Le redémarrage automatique ne s’applique qu’à un `.app`, pas à `cargo run`.

## Release

```sh
./release.sh            # version corrective suivante
./release.sh minor      # ou major, ou X.Y.Z
./release.sh current    # première publication : tague la version de Cargo.toml
./release.sh --no-push  # commit et tag locaux seulement
```

`./release.sh` lance les vérifications (`fmt`, `clippy`, tests Rust et Python), écrit la version dans `Cargo.toml`, `Cargo.lock` et `bundle/Info.plist`, crée le commit `release vX.Y.Z` et son tag, puis pousse les deux vers le remote Git qui pointe sur `github.com` (`RELEASE_REMOTE=<remote>` pour en imposer un). Il ne construit rien.

Le tag lance `.github/workflows/release.yml` sur `macos-latest` : build universel, signature Developer ID avec runtime renforcé, notarisation et agrafage du `.app` puis du DMG, évaluation Gatekeeper, publication de `StatusBadge.dmg` dans la release GitHub du tag. Le même workflow lancé à la main (`workflow_dispatch`, `gh workflow run release`) fait tout sauf publier ; le DMG est alors un artefact du run. À lancer avant de taguer après une modification du workflow ou de `bundle/`.

Secrets du dépôt GitHub, tous obligatoires (un secret absent fait échouer le job) :

| Secret | Valeur |
| --- | --- |
| `CODESIGN_CERT_P12_BASE64` | Certificat Developer ID Application avec sa clé privée, exporté en `.p12` depuis Trousseaux d’accès, encodé en base64. Il doit correspondre à l’identité nommée dans `bundle/release-config.sh`. |
| `CODESIGN_CERT_PASSWORD` | Mot de passe choisi à l’export du `.p12`. |
| `NOTARY_APPLE_ID` | Identifiant Apple du compte développeur. |
| `NOTARY_TEAM_ID` | Identifiant d’équipe Apple. |
| `NOTARY_PASSWORD` | Mot de passe pour application créé sur account.apple.com. |

`bundle/bundle.sh`, `bundle/dmg.sh` et `bundle/notarize.sh` fonctionnent aussi en local pour un essai ponctuel : signature avec le certificat du Trousseau, notarisation avec le profil `notarytool` nommé dans `bundle/release-config.sh`.

## Vérification manuelle avant publication

1. Ajout d’un sudo de production puis de staging : bulbe, oreille, centre combiné.
2. Rejet d’un lien `statusbadge://` sans demande active, d’un nonce expiré, d’un jeton refusé par le résumé.
3. 401/403 sur un sudo sans effet sur les autres ; retrait d’un sudo : Trousseau nettoyé.
4. Pulsation rouge, Réduire les animations, données périmées.
5. Drag sans ouvrir le navigateur, clic simple, position restaurée au redémarrage.
6. Autre application active, plusieurs Spaces, plein écran, écran externe puis débranchement.
7. Clic droit → **Rechercher une mise à jour** : « … » puis « ✓ » ou « ! », qui disparaît après quelques secondes. `--demo` : le menu contient toutes ses lignes sans coupure.
8. Avec une release plus récente que la version installée : flèche, téléchargement, ouverture du DMG, remplacement dans Applications, redémarrage du badge sur la nouvelle version en moins d’une minute, sudos toujours connectés.
