//! Privacy policy page (`/privacy`), required by the Play Store listing (K14), and the
//! index page.

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The privacy policy, French then English. `controller`: who is responsible (name of
/// the publisher); `contact`: e-mail address for requests.
pub fn page(controller: &str, contact: &str, public_url: &str) -> String {
    let who = escape(controller);
    let c = escape(contact);
    let u = escape(public_url);
    format!(
        r#"<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>skimap — politique de confidentialité</title>
<style>body{{font:16px/1.5 system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem;color:#222}}h1,h2{{line-height:1.2}}code{{background:#f3f3f3;padding:0 .2em}}</style>
</head>
<body>
<h1>skimap — politique de confidentialité</h1>
<p>Mise à jour : 15 novembre 2026. Responsable : {who}, contact : <a href="mailto:{c}">{c}</a>.</p>

<h2>En bref</h2>
<ul>
<li>L'application fonctionne hors ligne : cartes, itinéraires, temps estimés et conseils sont calculés sur le téléphone. Ni compte, ni publicité, ni mesure d'audience, ni revente de données.</li>
<li>Le serveur <code>{u}</code> ne fait que distribuer les cartes et leurs mises à jour, et, seulement si vous les utilisez, l'alerte en cas de non-retour et la connexion à Strava.</li>
</ul>

<h2>Téléchargement des cartes</h2>
<p>Pour télécharger ou mettre à jour une zone, le téléphone demande des fichiers au serveur. Comme tout site web, la requête transporte l'adresse IP du téléphone. Le serveur n'enregistre pas les adresses IP ; ses journaux techniques ne gardent que le chemin demandé, le code de réponse, la taille et la durée, quelques jours. Le serveur est derrière Cloudflare (réseau de diffusion), qui traite ces requêtes selon sa propre politique (<a href="https://www.cloudflare.com/privacypolicy/">cloudflare.com/privacypolicy</a>). Hébergement : Oracle Cloud, région de Paris.</p>

<h2>Alerte en cas de non-retour (facultative)</h2>
<p>Si vous l'activez avant une sortie, le téléphone envoie au serveur : le nom que vous indiquez, éventuellement votre numéro de téléphone, les adresses e-mail de un à trois contacts, l'heure de retour prévue, la description de l'itinéraire, l'itinéraire prévu (points) et, si vous le permettez, des positions envoyées pendant la sortie quand il y a du réseau. Ces données ne servent qu'à une chose : si vous ne confirmez pas votre retour à temps, le serveur envoie un e-mail à vos contacts avec ces informations ; il leur écrit à nouveau quand vous confirmez votre retour. Elles sont envoyées par un service d'envoi d'e-mails (relais SMTP du compte d'expédition) et ne sont communiquées à personne d'autre. Elles sont effacées automatiquement 7 jours après la fin de la sortie (retour confirmé, annulation ou alerte), et vous pouvez annuler une sortie à tout moment depuis l'application. Base légale : votre consentement, que vous donnez en activant l'alerte. Prévenez vos contacts que vous les désignez.</p>

<h2>Strava (facultatif)</h2>
<p>Si vous connectez Strava pour y envoyer vos sorties, l'autorisation se fait sur le site ou l'application Strava. Le serveur ne sert qu'à échanger le code d'autorisation contre des jetons, parce que Strava exige pour cela un secret qui ne peut pas être dans l'application. Les jetons passent par le serveur sans y être enregistrés ni journalisés, et restent sur votre téléphone. skimap n'importe pas vos données Strava par l'API. Vous pouvez déconnecter Strava depuis l'application ou depuis les réglages de Strava.</p>

<h2>Sur le téléphone</h2>
<p>Votre position, vos traces et vos réglages restent sur le téléphone ; ils ne sont envoyés nulle part, sauf ce qui est décrit ci-dessus pour l'alerte. Les fichiers que vous importez (par exemple un export de vos activités Strava) sont lus sur le téléphone.</p>

<h2>Vos droits</h2>
<p>Vous pouvez demander l'accès, la rectification ou l'effacement de vos données, ou vous opposer à leur traitement, en écrivant à <a href="mailto:{c}">{c}</a>. Vous pouvez aussi saisir la CNIL (<a href="https://www.cnil.fr">cnil.fr</a>).</p>

<hr>
<h1 lang="en">Privacy policy (English)</h1>
<div lang="en">
<p>Updated 15 November 2026. Controller: {who}, <a href="mailto:{c}">{c}</a>.</p>
<ul>
<li>The app works offline: maps, routes, time estimates and advice are computed on the phone. No account, no ads, no analytics, no sale of data.</li>
<li>Map downloads: requests carry the phone's IP address, which the server does not record (technical logs keep path, status, size and duration for a few days). The server sits behind Cloudflare, which processes these requests under its own policy. Hosting: Oracle Cloud, Paris region.</li>
<li>Non-return alert (optional): the name you give, optionally your phone number, one to three contact e-mail addresses, your planned return time, the route description and points, and positions sent during the trip when there is coverage. Used only to e-mail your contacts if you do not confirm your return in time, and again when you do. Sent through an e-mail relay, shared with nobody else, deleted automatically 7 days after the trip ends; you can cancel at any time. Legal basis: your consent.</li>
<li>Strava (optional): the server only exchanges the authorization code for tokens, because Strava requires a secret that cannot ship in the app. Tokens pass through without being stored or logged and stay on your phone. No Strava data is imported through the API.</li>
<li>Your position, tracks and settings stay on the phone except as described for the alert.</li>
<li>Rights of access, rectification, erasure and objection: write to <a href="mailto:{c}">{c}</a>; you may also complain to your data protection authority.</li>
</ul>
</div>
</body>
</html>
"#
    )
}

/// The index page.
pub fn index() -> String {
    r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><title>skimap</title>
<meta name="viewport" content="width=device-width, initial-scale=1"></head>
<body style="font:16px/1.5 system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem">
<h1>skimap</h1>
<p>Serveur de distribution de l'application skimap (cartes hors ligne de ski de fond et de ski de randonnée nordique).</p>
<ul>
<li><a href="/v1/catalog">Catalogue des zones</a> (JSON)</li>
<li><a href="/privacy">Politique de confidentialité</a></li>
</ul>
</body></html>
"#
    .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn contact_is_escaped() {
        let p = super::page("Éditeur", "a<b@c.fr", "https://x");
        assert!(p.contains("a&lt;b@c.fr") && !p.contains("a<b@c.fr"));
        assert!(p.contains("7 jours"));
    }
}
