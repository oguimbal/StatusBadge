# Protocole Status Badge v1

Contrat entre l’application macOS Status Badge et une interface d’administration (« sudo ») qui veut y être enregistrée. Le badge ne connaît aucun sudo à l’avance : l’utilisateur saisit l’URL HTTPS de l’interface, le sudo fournit lui-même son nom, son environnement, l’URL de son résumé et un jeton.

Deux éléments à implémenter côté projet :

1. une route navigateur `<base>/statusbadge`, derrière la session d’administration ;
2. un endpoint HTTP de résumé, authentifié par `Authorization: Bearer <jeton>`.

## 1. Connexion

`<base>` est l’origine de l’URL saisie dans le badge (schéma, hôte, port) : `https://sudo.example.com`. Un chemin collé avec l’URL (`/dashboard`) est ignoré : la route `/statusbadge` est toujours à la racine du sudo. HTTPS obligatoire ; HTTP accepté uniquement pour `localhost`, `*.localhost`, `127.0.0.1` et `[::1]` (serveurs de développement).

### 1.1 Requête du badge

Le badge tire 32 octets aléatoires et ouvre dans le navigateur :

```
GET <base>/statusbadge?state=<64 caractères hexadécimaux>
```

### 1.2 Page du sudo

La route reste derrière l’authentification normale du sudo : sans session, le sudo affiche son login puis revient sur cette route avec la même requête.

Une fois l’administrateur connecté, la page :

- refuse toute requête qui ne contient pas exactement un paramètre `state` de 64 caractères hexadécimaux (pas de `redirect`, pas de doublon) ;
- obtient un jeton de lecture du résumé (§3) ;
- retire `state` de la barre d’adresse (`history.replaceState`) puis navigue vers le lien de retour ;
- affiche un bouton « Ouvrir Status Badge » qui relance le même lien, et un message d’erreur factuel si la demande est invalide.

Le sudo ne stocke pas `state` : il le renvoie tel quel, le badge seul le vérifie.

### 1.3 Lien de retour

```
statusbadge://connect#v=1&state=…&token=…&name=…&environment=…&summary=…&open=…[&expires=…]
```

Le fragment est encodé en `application/x-www-form-urlencoded` (`new URLSearchParams({...}).toString()`). Le jeton n’apparaît jamais dans une requête HTTP ni dans une redirection arbitraire.

| Clé | Obligatoire | Valeur |
| --- | --- | --- |
| `v` | oui | `1` |
| `state` | oui | valeur reçue, inchangée |
| `token` | oui | jeton opaque, 1 à 12 000 caractères parmi `A-Z a-z 0-9 . _ ~ + / = -` |
| `name` | oui | nom affiché du projet, 1 à 40 caractères, sans caractère de contrôle |
| `environment` | oui | `production`, `staging` ou `development` |
| `summary` | oui | URL absolue de l’endpoint de résumé (§2) |
| `open` | oui | URL ouverte au clic sur le satellite, de préférence une route qui choisit la rubrique selon le statut |
| `expires` | non | expiration du jeton, en secondes Unix |

Aucune autre clé, aucun doublon. `summary` et `open` suivent la même règle de schéma que `<base>` ; leur domaine n’est pas contraint (un résumé peut être servi par un autre domaine que l’interface d’administration).

### 1.4 Vérifications du badge

Le badge accepte un retour seulement si :

- schéma `statusbadge`, hôte `connect`, sans utilisateur, port, chemin ni requête ;
- `state` correspond à une demande en cours, de moins de 10 minutes, utilisée une seule fois (même en cas d’échec) ;
- toutes les valeurs respectent le tableau ci-dessus ;
- un premier `GET summary` avec le jeton renvoie un résumé valide.

Il enregistre alors le jeton dans le Trousseau macOS (service `fr.gmbl.statusbadge.session`, compte = identifiant local de la cible) et la cible dans ses préférences (`base`, `name`, `environment`, `summary`, `open`, sans secret). Une reconnexion à une `base` déjà enregistrée remplace la cible existante.

## 2. Résumé

```
GET <summary>
Authorization: Bearer <token>
```

Réponse `200`, `Content-Type: application/json`, `Cache-Control: no-store`, au plus 64 Kio :

```json
{
  "updatedAt": 1761825600000,
  "status": "degraded",
  "count": 3,
  "sections": [
    { "id": "api", "label": "API", "status": "operational", "count": 0 },
    { "id": "jobs", "label": "Tâches", "status": "degraded", "count": 3 }
  ]
}
```

- `status` ∈ `operational`, `unknown`, `watch`, `degraded`, `outage`. Ordre de gravité : `operational` < `unknown` < `watch` < `degraded` < `outage`.
- `status` et `count` globaux sont calculés par le serveur ; le badge ne les recalcule pas à partir des rubriques. `count` = nombre d’alertes ou d’incidents actifs, chacun compté une fois.
- `updatedAt` : date des données en millisecondes Unix, ou `null`. Au-delà de 30 secondes, le badge n’affiche jamais vert.
- `sections` : au plus 64, dans l’ordre d’affichage. `id` : 1 à 64 caractères `A-Z a-z 0-9 - _`, unique. `label` : 1 à 120 caractères, sans caractère de contrôle.
- Les champs supplémentaires sont ignorés.

Codes :

- `401` ou `403` : jeton refusé ou expiré. Le badge efface le jeton et affiche un cadenas.
- Autre erreur, `429`, `5xx`, réponse invalide ou réseau indisponible : la session est conservée, les dernières données sont marquées périmées.
- Les redirections ne sont pas suivies.

L’endpoint ne doit pas exiger d’en-tête `Origin` : le badge n’en envoie pas. Le badge interroge toutes les 3 secondes après la réponse précédente.

## 3. Jeton

Le jeton est opaque pour le badge. Il doit :

- ne permettre que la lecture du résumé (pas la session d’administration complète) ;
- rester révocable : le serveur vérifie à chaque requête que l’administrateur existe encore et garde ses droits, ou que le jeton n’a pas été révoqué ;
- expirer (90 jours au plus conseillé). Le badge affiche le cadenas à l’expiration indiquée par `expires` ou au premier refus.

Forme conseillée sans stockage : JWT HS256 signé par la clé existante du serveur, avec une audience dédiée (`statusbadge`) que le reste de l’API refuse, `sub` = identifiant de l’administrateur, `exp`.

Le jeton d’administration existant peut servir de transition lorsqu’il est déjà limité dans le temps.

## 4. Test manuel d’un sudo

1. Ouvrir `<base>/statusbadge?state=` + 64 caractères hexadécimaux sans session : le login s’affiche, puis le retour vers la route.
2. Avec session : le navigateur propose d’ouvrir Status Badge ; le lien contient tous les champs du §1.3.
3. `curl -H "Authorization: Bearer <jeton>" <summary>` renvoie le résumé ; sans jeton ou avec un jeton modifié : `401` ou `403`.
4. Le jeton refuse les autres routes d’administration.
