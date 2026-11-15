# Alerte de non-retour côté serveur (S2, #60) : complément facultatif

[← skimap-server](../README.md)

Règle de l'utilisateur : tout ce qui est critique est sur le téléphone, sans connexion internet. L'alerte principale est **sur le téléphone** (K10, #61) : le plan part au contact par SMS au départ, le contact sert de minuterie, les positions partent par SMS quand le GSM passe. Le service du serveur ne couvre que le cas où le téléphone ne peut plus rien envoyer (batterie, chute, téléphone perdu) : il est gardé minimal (minuterie, envoi, surveillance de la minuterie).

## Canal : e-mail par un relais SMTP gratuit

Ce qui existe déjà dans l'infra (lu le 15/11/2026) :

| Canal | Où | Destinataires | Coût | Retenu |
|---|---|---|---|---|
| Relais SMTP Gmail (`smtp.gmail.com:587`, STARTTLS, mot de passe d'application) | formulaire de contact d'un autre service, même cluster (`secret SMTP d'un autre service`) | n'importe quelle adresse | gratuit ; 500 destinataires par jour pour un compte Gmail gratuit | **oui** |
| Cloudflare Email Service (`/email/sending/send`) | autre service, secret `<secret>` | seulement des adresses de destination **vérifiées** dans le compte Cloudflare (chaque contact devrait cliquer un lien de Cloudflare) ; envoi pas activé sur le compte (sonde « NOT READY ») | gratuit vers les adresses vérifiées | non |
| Email Routing + `send_email` d'un Worker | Worker de contact | adresses vérifiées seulement | gratuit | non |
| SMS (Twilio, OVH…) | rien | tous | payant, environ 0,05 à 0,08 € par SMS | non (service payant : décision du propriétaire) |
| Notification push (FCM, ntfy) | rien | il faudrait que le contact installe une app | gratuit | non |

Choix : **e-mail par le relais SMTP d'un compte Gmail gratuit dédié** (par exemple un compte « alertes skimap »), comme un autre service du même cluster. Aucun coût, aucun service payant ; le compte et son mot de passe d'application sont à créer (secret `skimap-smtp`). Un compte Google Workspace existant marcherait aussi (2 000 destinataires par jour), mais un compte Workspace de plus serait payant.

Limites : un e-mail n'est pas un SMS (le contact peut ne le lire que plus tard) ; 500 destinataires par jour au plus (plafond du service : 400 sur 24 h, réglable) ; délivrabilité d'un compte Gmail gratuit (pas de SPF ni DKIM du domaine warnier.net ; l'expéditeur est le compte Gmail) ; abus possible (un tiers peut faire envoyer une alerte à une adresse quelconque, après le délai de grâce) : bornes de 10 plans par heure et par adresse IP, 500 par jour en tout, 3 contacts, texte borné, en-têtes protégés contre l'injection.

## Fonctionnement

- Minuterie : un fil appelle `Alerts::tick` toutes les 15 s ; les tests la pilotent avec une horloge simulée (`clock::ManualClock`, `tests/alerts.rs`).
- File d'envoi dans SQLite, une ligne par contact, nouvelles tentatives (1, 2, 4, 8, 16 puis 30 min), abandon après 24 h ; retour confirmé après l'alerte : message « rentré(e), alerte levée » aux contacts déjà prévenus, alertes pas encore parties annulées.
- Contenu (français, heure de Paris) : nom, téléphone du skieur s'il est donné, échéance et délai, plan, secteur, départ et arrivée prévus, points de l'itinéraire, dernière position reçue avec lien OpenStreetMap, rappel du 112.
- Données effacées 7 jours après la fin de la sortie.

## Surveillance

`/healthz` répond 503 quand la minuterie n'a pas tourné depuis 2 minutes (ou si SQLite ne répond plus) : la sonde de vie de Kubernetes redémarre alors le pod, et ArgoCD montre l'app `skimap` en mauvaise santé. Rien n'envoie d'alerte vers l'extérieur.

Option non retenue (non implémentée, règle de l'utilisateur) : un second chemin indépendant du serveur (minuterie Cloudflare Worker et stockage Cloudflare, ou un second cluster en secours), avec un contrôle extérieur de `/healthz` qui préviendrait le propriétaire.

## Preuves (15/11/2026)

- `tests/alerts.rs` (horloge simulée, HTTP réel) : aucune alerte avant échéance + 30 min, une par contact à l'heure dite, rien de plus aux tours suivants ; message vérifié (heure de Paris, plan, positions, liens) ; retour après l'alerte : deux messages « rentré(e) » ; retour à temps, prolongation, annulation : aucune alerte ; échecs d'envoi réessayés, alerte non partie annulée au retour, abandon après 24 h ; effacement après 7 jours ; plans invalides refusés (adresses, injection d'en-tête, échéances, délai, site, coordonnées) ; limite de 10 plans par heure et par client.
- `tests/smtp.rs` : faux relais (AUTH PLAIN, MAIL, RCPT, DATA en Base64, sujet encodé) ; erreurs 535 et 550 ; chaîne complète plan → minuterie → relais ; poignée de main STARTTLS réelle avec `smtp.gmail.com:587` (AUTH PLAIN proposé après TLS).
- Critère « alerte de test reçue par un contact » : à faire quand le compte d'envoi existe (`skimap-server send-test-email <adresse>`, puis un plan avec échéance à 5 minutes et délai 0).
