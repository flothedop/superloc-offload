/* ==========================================================================
   SuperLoc Offload — logique d'interface
   ========================================================================== */

const $ = (id) => document.getElementById(id);

// Le message d'accueil du journal est retiré du DOM pendant une copie :
// on en garde la référence pour pouvoir le remettre ensuite.
const messageVide = document.getElementById('journal-vide');

const etat = {
  source: null,
  disques: [],
  lignes: [],
  enCours: false,
  rapport: null,
  // Débit, courbe et avancement de chaque destination.
  mesures: [],
  // Un verdict affiché (vert, rouge, ou refus) reste à l'écran tant que
  // l'utilisateur n'a pas relancé une opération : c'est la seule chose
  // qu'il est venu lire, aucun rafraîchissement d'état ne doit l'effacer.
  verdictAffiche: false,
};

/* ------------------------------------------------------------------ Outils */

function debitLisible(o) {
  if (!o || o <= 0) return '—';
  const mo = o / 1e6;
  if (mo >= 1000) return `${(mo / 1000).toFixed(2)} Go/s`;
  if (mo >= 100) return `${mo.toFixed(0)} Mo/s`;
  return `${mo.toFixed(1)} Mo/s`;
}

function octets(n) {
  if (n === 0) return '0 o';
  const u = ['o', 'ko', 'Mo', 'Go', 'To'];
  let v = n, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return `${i === 0 ? v : v.toFixed(v < 10 ? 2 : v < 100 ? 1 : 0)} ${u[i]}`;
}

function codeClip(clip) {
  const m = clip.match(/_(C\d+)_/) || clip.match(/^(C\d+)/);
  return m ? m[1] : clip.replace(/\.[A-Za-z0-9]+$/, '');
}

function nomCourt(chemin) {
  const bouts = chemin.split('/');
  return bouts[bouts.length - 1];
}

/* ------------------------------------------------------------ Bandeau d'état */

function bandeau({ etat: e, verdict, detail, pourcent }) {
  const b = $('bandeau');
  b.dataset.etat = e;
  $('verdict').textContent = verdict;
  $('detail').textContent = detail;
  const enCours = e === 'copie' || e === 'verif';
  $('chiffre').classList.toggle('cache', !enCours);
  if (pourcent != null) {
    $('remplissage').style.width = `${pourcent}%`;
    $('chiffre').innerHTML = `${Math.round(pourcent)}<span>%</span>`;
  }
  if (!enCours) $('remplissage').style.width = '0';
}

function bandeauRepos() {
  if (etat.enCours || etat.verdictAffiche) return;
  if (!etat.source) {
    bandeau({ etat: 'repos', verdict: 'Prêt à décharger', detail: 'Choisir une carte et au moins un disque.' });
  } else if (etat.disques.length === 0) {
    bandeau({ etat: 'repos', verdict: 'Prêt à décharger', detail: 'Ajouter au moins un disque de destination.' });
  } else {
    const manque = etat.disques.filter((d) => d.libre < etat.source.octets);
    if (manque.length) {
      bandeau({
        etat: 'repos',
        verdict: 'Place insuffisante',
        detail: `${manque.map((d) => d.nom).join(', ')} ne peut pas recevoir ${octets(etat.source.octets)}.`,
      });
    } else {
      bandeau({
        etat: 'repos',
        verdict: 'Prêt à décharger',
        detail: `${etat.source.fichiers.length} fichiers · ${octets(etat.source.octets)} vers ${etat.disques.length} disque${etat.disques.length > 1 ? 's' : ''}.`,
      });
    }
  }
}

/* -------------------------------------------------------------------- Carte */

function majSource() {
  const vide = !etat.source;
  $('carte-vide').classList.toggle('cache', !vide);
  $('carte-pleine').classList.toggle('cache', vide);
  $('source-structure').textContent = vide ? '' : etat.source.structure;
  if (!vide) {
    $('carte-nom').textContent = etat.source.nom;
    $('carte-chemin').textContent = etat.source.chemin;
    $('fait-clips').textContent = etat.source.clips;
    $('fait-fichiers').textContent = etat.source.fichiers.length;
    $('fait-taille').textContent = octets(etat.source.octets);
  }
  majAction();
}

/* -------------------------------------------------------------- Destinations */

function majDisques() {
  const hote = $('disques');
  hote.textContent = '';

  etat.disques.forEach((d, i) => {
    const besoin = etat.source ? etat.source.octets : 0;
    const occupe = ((d.total - d.libre) / d.total) * 100;
    const ajout = Math.min(100 - occupe, (besoin / d.total) * 100);
    const tient = d.libre >= besoin;

    const ligne = document.createElement('div');
    ligne.className = 'disque';
    ligne.innerHTML = `
      <div class="disque__rang mono">${i + 1}</div>
      <div>
        <div class="disque__nom"></div>
        <div class="disque__chemin mono"></div>
        <div class="jauge ${tient ? '' : 'jauge--pleine'}">
          <div class="jauge__occupe" style="width:${occupe}%"></div>
          <div class="jauge__ajout" style="left:${occupe}%;width:${Math.max(ajout, besoin ? 0.6 : 0)}%"></div>
        </div>
        <div class="disque__espace ${tient ? '' : 'insuffisant'}"></div>
        <div class="disque__mesure cache" data-mesure="${i}">
          <span class="disque__debit">—</span>
          <svg class="courbe" viewBox="0 0 100 22" preserveAspectRatio="none" aria-hidden="true">
            <polyline points=""></polyline>
          </svg>
          <span class="disque__avancement"></span>
        </div>
      </div>
      <button class="disque__retirer" type="button" aria-label="Retirer ce disque">×</button>`;
    ligne.querySelector('.disque__nom').textContent = d.nom;
    ligne.querySelector('.disque__chemin').textContent = d.chemin;
    ligne.querySelector('.disque__espace').textContent = besoin
      ? (tient
        ? `${octets(d.libre)} libres — il restera ${octets(d.libre - besoin)} après la copie`
        : `${octets(d.libre)} libres — il manque ${octets(besoin - d.libre)}`)
      : `${octets(d.libre)} libres sur ${octets(d.total)}`;
    ligne.querySelector('.disque__retirer').addEventListener('click', () => {
      if (etat.enCours) return;
      etat.disques.splice(i, 1);
      majDisques();
    });
    hote.appendChild(ligne);
  });

  $('compte-disques').textContent = `${etat.disques.length} sur 3`;
  $('ajouter').disabled = etat.disques.length >= 3 || etat.enCours;
  $('ajouter').textContent = etat.disques.length >= 3 ? 'Trois disques au maximum' : 'Ajouter un disque';
  document.documentElement.style.setProperty(
    '--colonnes-dest',
    etat.disques.length ? `${etat.disques.length * 21}px` : '64px');
  const entete = $('entete-dest');
  entete.textContent = '';
  if (etat.disques.length) {
    etat.disques.forEach((d, i) => {
      const rep = document.createElement('span');
      rep.className = 'repere';
      rep.textContent = i + 1;
      rep.title = d.nom;
      entete.appendChild(rep);
    });
  } else {
    entete.textContent = 'Disques';
  }
  majAction();
}

function majAction() {
  const prêt = !!etat.source
    && etat.disques.length > 0
    && etat.disques.every((d) => d.libre >= etat.source.octets);
  $('btn-decharger').disabled = !prêt || etat.enCours || !!etat.rapport;
  bandeauRepos();
}

function majMesure(destination, debit, fichiersFaits) {
  const m = etat.mesures[destination] || (etat.mesures[destination] = { courbe: [], max: 0 });
  m.debit = debit;
  m.faits = fichiersFaits;
  if (debit > 0) {
    m.courbe.push(debit);
    if (m.courbe.length > 48) m.courbe.shift();
    m.max = Math.max(m.max, debit);
  }

  const bloc = document.querySelector(`[data-mesure="${destination}"]`);
  if (!bloc) return;
  bloc.classList.remove('cache');
  bloc.querySelector('.disque__debit').textContent = debitLisible(debit);
  const total = etat.source ? etat.source.fichiers.length : 0;
  bloc.querySelector('.disque__avancement').textContent =
    total ? `${fichiersFaits || 0}/${total} vérifiés` : '';

  // Courbe normalisée sur le maximum vu : ce qu'on lit, c'est la régularité.
  const pts = m.courbe;
  if (pts.length > 1 && m.max > 0) {
    const pas = 100 / (pts.length - 1);
    bloc.querySelector('polyline').setAttribute(
      'points',
      pts.map((v, i) => `${(i * pas).toFixed(1)},${(21 - (v / m.max) * 19).toFixed(1)}`).join(' '),
    );
  }
}

function cacherMesures() {
  document.querySelectorAll('[data-mesure]').forEach((b) => b.classList.add('cache'));
}

/* --------------------------------------------------- Analyse de lenteur */

function afficherAnalyse(diag) {
  if (!diag || (!diag.faits.length && !diag.hypotheses.length)) return;
  const section = $('analyse');
  const faits = $('analyse-faits');
  const hypos = $('analyse-hypotheses');

  faits.textContent = '';
  diag.faits.forEach((f) => {
    const li = document.createElement('li');
    li.textContent = f;
    faits.appendChild(li);
  });

  hypos.textContent = '';
  diag.hypotheses.forEach((h) => {
    const li = document.createElement('li');
    const titre = document.createElement('span');
    titre.className = 'hypo__titre';
    titre.textContent = h.titre;
    const parce = document.createTextNode(`parce que ${h.parce_que}`);
    const test = document.createElement('span');
    test.className = 'hypo__test';
    test.textContent = `Pour trancher : ${h.verifier}`;
    li.append(titre, parce, test);
    hypos.appendChild(li);
  });

  section.classList.remove('cache');
}

$('analyse-fermer').addEventListener('click', () => $('analyse').classList.add('cache'));

/* ------------------------------------------------------------------ Journal */

function journalPreparer() {
  const corps = $('journal-corps');
  corps.textContent = '';
  etat.premierEchec = null;
  etat.lignes = etat.source.fichiers.map((f, i) => {
    const el = document.createElement('div');
    el.className = 'ligne';
    el.innerHTML = `
      <div class="ligne__fichier"></div>
      <div class="ligne__clip"></div>
      <div class="ligne__taille">${octets(f.taille)}</div>
      <div class="ligne__etats">${etat.disques.map(() => '<span class="pastille" data-etat="attente"></span>').join('')}</div>`;
    el.querySelector('.ligne__fichier').textContent = nomCourt(f.chemin);
    // Un fichier qui constitue à lui seul son clip n'a pas besoin d'être
    // répété dans la colonne : son nom est déjà en première colonne.
    el.querySelector('.ligne__clip').textContent =
      f.clip === f.chemin ? '—' : codeClip(f.clip);
    corps.appendChild(el);
    return el;
  });
  messageVide.classList.add('cache');
}

function journalEcriture(index) {
  const l = etat.lignes[index];
  if (!l) return;
  l.querySelectorAll('.pastille').forEach((p) => { p.dataset.etat = 'ecriture'; });
  l.scrollIntoView({ block: 'nearest' });
}

function journalResultat(r) {
  const l = etat.lignes[r.index];
  if (!l) return;
  // Chaque destination remonte son propre verdict, quand elle y arrive.
  const pastilles = l.querySelectorAll('.pastille');
  const p = pastilles[r.destination];
  if (p) {
    p.dataset.etat = r.etat;
    p.textContent = r.etat === 'ok' ? '✓' : '✕';
  }
  if (r.etat === 'echec') {
    l.classList.add('ligne--echec');
    // Le motif n'est écrit qu'une fois : répété à chaque ligne, il noie
    // justement l'information qu'il porte.
    if (r.motif && etat.premierEchec == null) {
      etat.premierEchec = r.index;
      const m = document.createElement('div');
      m.className = 'ligne__motif';
      m.textContent = r.motif;
      l.appendChild(m);
    }
  }
}

/* ------------------------------------------------------------- Déchargement */

async function decharger(scenario) {
  etat.enCours = true;
  etat.rapport = null;
  etat.verdictAffiche = false;
  etat.mesures = etat.disques.map(() => ({ courbe: [], max: 0 }));
  $('analyse').classList.add('cache');
  majDisques();
  $('btn-decharger').disabled = true;
  $('btn-verifier').disabled = true;
  ['btn-dossier', 'btn-rapport', 'btn-recommencer'].forEach((b) => $(b).classList.add('cache'));
  journalPreparer();

  const debut = Date.now();
  let dernierChemin = '';

  let rapport;
  try {
    rapport = await Backend.decharger({
      source: etat.source,
      destinations: etat.disques,
      empreinte: $('empreinte').value,
      verifierSource: $('verifier-source').checked,
      scenario,
    }, {
      onFichier: (e) => {
        dernierChemin = e.chemin;
        journalEcriture(e.index);
        $('pied-info').textContent = `Fichier ${e.index + 1} sur ${e.total} — ${nomCourt(e.chemin)}`;
      },
      onOctets: (e) => {
        const pct = (e.copies / e.total) * 100;
        const ecoule = (Date.now() - debut) / 1000;
        const moyen = ecoule > 0.5 ? e.copies / ecoule : 0;
        const restant = moyen > 0 ? (e.total - e.copies) / moyen : 0;
        bandeau({
          etat: 'copie',
          verdict: 'Déchargement',
          detail: moyen
            ? `carte ${debitLisible(e.debit || moyen)} — ${Math.max(1, Math.ceil(restant / 60))} min restantes — ${nomCourt(dernierChemin)}`
            : nomCourt(dernierChemin),
          pourcent: pct,
        });
      },
      // Chaque destination remonte son propre débit : c'est elle qu'on
      // regarde quand on se demande qui traîne.
      onDebit: (e) => majMesure(e.destination, e.debit, e.fichiers_faits),
      onDiagnostic: afficherAnalyse,
      onResultat: journalResultat,
    });
  } catch (e) {
    // Refus au démarrage : dossier déjà présent, place insuffisante, carte
    // illisible. L'écran doit le dire, pas rester sur « Déchargement ».
    etat.enCours = false;
    etat.rapport = null;
    etat.verdictAffiche = true;
    bandeau({
      etat: 'alerte',
      verdict: 'Copie refusée',
      detail: String(e && e.message ? e.message : e),
    });
    $('pied-info').textContent = 'Aucun fichier copié';
    $('btn-verifier').disabled = false;
    majDisques();
    majAction();
    return;
  }

  etat.enCours = false;
  etat.rapport = rapport;
  terminer(rapport);
}

function terminer(rapport) {
  etat.verdictAffiche = true;
  const echecs = rapport.destinations.filter((d) => !d.conforme);
  if (rapport.conforme) {
    bandeau({
      etat: 'conforme',
      verdict: 'Copie conforme',
      detail: `${rapport.nb_fichiers} fichiers relus sur ${rapport.destinations.length} disque${rapport.destinations.length > 1 ? 's' : ''} — la carte peut être effacée.`,
    });
  } else {
    bandeau({
      etat: 'alerte',
      verdict: 'Copie non conforme',
      detail: `Ne pas effacer la carte. ${echecs.map((d) => `${d.nom} : ${d.fichiers_echec} fichiers en échec`).join(' · ')}`,
    });
  }
  $('pied-info').textContent = rapport.destinations
    .map(
      (d) =>
        `${d.nom} — ${d.fichiers_ok}/${rapport.nb_fichiers} vérifiés · ${debitLisible(d.debit_moyen)}`,
    )
    .join('   ·   ');
  if (rapport.diagnostic) afficherAnalyse(rapport.diagnostic);
  cacherMesures();
  // En cas d'échec, l'écran se place sur le premier fichier fautif plutôt
  // que de laisser le journal en bas de liste.
  if (!rapport.conforme && etat.premierEchec != null && etat.lignes[etat.premierEchec]) {
    etat.lignes[etat.premierEchec].scrollIntoView({ block: 'center' });
  }
  ['btn-dossier', 'btn-rapport', 'btn-recommencer'].forEach((b) => $(b).classList.remove('cache'));
  $('btn-verifier').disabled = false;
  majDisques();
}

/* ---------------------------------------------------------------- Câblage */

$('carte-vide').addEventListener('click', async () => {
  const c = await Backend.choisirCarte();
  if (c) {
    etat.source = c;
    etat.rapport = null;
    etat.verdictAffiche = false;
    majSource();
    majDisques();
    journalPreparer();
  }
});

$('ajouter').addEventListener('click', async () => {
  const d = await Backend.choisirDisque();
  if (d && !etat.disques.some((x) => x.chemin === d.chemin)) {
    etat.disques.push(d);
    majDisques();
  }
});

$('btn-decharger').addEventListener('click', () => decharger());

$('btn-recommencer').addEventListener('click', () => {
  etat.rapport = null;
  etat.source = null;
  etat.lignes = [];
  etat.premierEchec = null;
  etat.verdictAffiche = false;
  etat.mesures = [];
  $('analyse').classList.add('cache');
  $('journal-corps').textContent = '';
  $('journal-corps').appendChild(messageVide);
  messageVide.classList.remove('cache');
  $('pied-info').textContent = 'Aucune copie en cours';
  ['btn-dossier', 'btn-rapport', 'btn-recommencer'].forEach((b) => $(b).classList.add('cache'));
  majSource();
  majDisques();
});

$('btn-dossier').addEventListener('click', () => {
  if (etat.rapport) Backend.ouvrirDossier(etat.rapport.destinations[0].dossier);
});
$('btn-rapport').addEventListener('click', () => {
  if (etat.rapport) Backend.ouvrirDossier(etat.rapport.dossier_rapport || etat.rapport.destinations[0].dossier);
});

// Glisser-déposer d'une carte sur la fenêtre
['dragenter', 'dragover'].forEach((e) => document.addEventListener(e, (ev) => {
  ev.preventDefault();
  document.body.classList.add('glisse');
}));
['dragleave', 'drop'].forEach((e) => document.addEventListener(e, (ev) => {
  ev.preventDefault();
  document.body.classList.remove('glisse');
}));

majSource();
majDisques();

/* ------------------------------------------------- Vérifier un disque */

$('btn-verifier').addEventListener('click', async () => {
  if (etat.enCours) return;
  etat.enCours = true;
  etat.rapport = null;
  etat.verdictAffiche = false;
  $('btn-verifier').disabled = true;
  $('btn-decharger').disabled = true;
  bandeau({ etat: 'verif', verdict: 'Vérification', detail: 'Relecture du disque à partir de son MHL', pourcent: 0 });

  let r;
  try {
    r = await Backend.verifierDossier();
  } catch (e) {
    etat.enCours = false;
    $('btn-verifier').disabled = false;
    etat.verdictAffiche = true;
    bandeau({ etat: 'alerte', verdict: 'Vérification impossible', detail: String(e && e.message ? e.message : e) });
    majAction();
    return;
  }

  etat.enCours = false;
  $('btn-verifier').disabled = false;
  if (!r) { majAction(); bandeauRepos(); return; }
  afficherVerification(r);
});

function afficherVerification(r) {
  etat.verdictAffiche = true;
  const corps = $('journal-corps');
  corps.textContent = '';
  messageVide.classList.add('cache');
  document.documentElement.style.setProperty('--colonnes-dest', '64px');
  $('entete-dest').textContent = 'État';

  const echecs = r.lignes.filter((l) => l.etat !== 'Ok');
  let premier = null;

  r.lignes.forEach((l) => {
    const el = document.createElement('div');
    el.className = 'ligne' + (l.etat === 'Ok' ? '' : ' ligne--echec');
    el.innerHTML = `
      <div class="ligne__fichier"></div>
      <div class="ligne__clip"></div>
      <div class="ligne__taille"></div>
      <div class="ligne__etats"><span class="pastille" data-etat="${l.etat === 'Ok' ? 'ok' : 'echec'}">${l.etat === 'Ok' ? '✓' : '✕'}</span></div>`;
    el.querySelector('.ligne__fichier').textContent = nomCourt(l.fichier);
    el.querySelector('.ligne__clip').textContent = codeClip(l.fichier);
    if (l.detail) {
      const m = document.createElement('div');
      m.className = 'ligne__motif';
      m.textContent = l.detail;
      el.appendChild(m);
    }
    corps.appendChild(el);
    if (l.etat !== 'Ok' && !premier) premier = el;
  });

  const conforme = echecs.length === 0 && !r.residus.length;
  const morceaux = [];
  if (echecs.length) morceaux.push(`${echecs.length} fichiers en échec`);
  if (r.absents.length) morceaux.push(`${r.absents.length} absents`);
  if (r.en_trop.length) morceaux.push(`${r.en_trop.length} présents hors MHL`);
  if (r.residus.length) morceaux.push(`${r.residus.length} copies interrompues`);

  bandeau({
    etat: conforme ? 'conforme' : 'alerte',
    verdict: conforme ? 'Disque conforme' : 'Disque non conforme',
    detail: conforme
      ? `${r.lignes.length} fichiers relus et conformes au MHL.`
      : morceaux.join(' · '),
  });
  $('pied-info').textContent = `${r.dossier} — ${r.lignes.length} fichiers listés au MHL`;
  ['btn-dossier', 'btn-recommencer'].forEach((b) => $(b).classList.remove('cache'));
  etat.rapport = { dossier_rapport: r.dossier, destinations: [{ dossier: r.dossier }], conforme };
  if (premier) premier.scrollIntoView({ block: 'center' });
}

/* Accès de démonstration : rejoue un état complet pour relire l'interface. */
window.__demo = async (nom, vitesse) => {
  etat.source = await Backend.choisirCarte();
  etat.disques = [await Backend.choisirDisque(), await Backend.choisirDisque()];
  majSource(); majDisques(); journalPreparer();
  if (nom === 'pret') return;
  Backend.vitesse = vitesse || 40;
  const scenarios = { echec: 'echec', lent: 'lent' };
  await decharger(scenarios[nom]);
};

/* -------------------------------------------------- Glisser-déposer réel
   Tauri remet les chemins des dossiers déposés, ainsi que la position du
   curseur : un dossier lâché sur la zone carte devient la carte, lâché sur
   la colonne des destinations il devient un disque. */
if (window.__TAURI__) {
  const dansZone = (id, p) => {
    const r = $(id).getBoundingClientRect();
    const f = window.devicePixelRatio || 1;
    const x = p.x / f;
    const y = p.y / f;
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };

  window.__TAURI__.event.listen('tauri://drag-enter', () => document.body.classList.add('glisse'));
  window.__TAURI__.event.listen('tauri://drag-leave', () => document.body.classList.remove('glisse'));
  window.__TAURI__.event.listen('tauri://drag-drop', async (e) => {
    document.body.classList.remove('glisse');
    if (etat.enCours) return;
    const chemin = (e.payload.paths || [])[0];
    if (!chemin) return;

    const surDestinations = dansZone('disques', e.payload.position)
      || dansZone('ajouter', e.payload.position);

    if (surDestinations && etat.source) {
      const d = await Backend.choisirDisque(chemin);
      if (d && !etat.disques.some((x) => x.chemin === d.chemin) && etat.disques.length < 3) {
        etat.disques.push(d);
        majDisques();
      }
    } else {
      const c = await Backend.choisirCarte(chemin);
      if (c) {
        etat.source = c;
        etat.rapport = null;
        etat.verdictAffiche = false;
        majSource();
        majDisques();
        journalPreparer();
      }
    }
  });
}

/* Essai automatique de bout en bout : OFFLOAD_AUTO="carte|disque1|disque2"
   déclenche un déchargement complet au démarrage, sans passer par les
   sélecteurs de dossier. Sert aux tests, jamais au fonctionnement normal. */
window.__essai = async (carte, disques) => {
  etat.source = await Backend.choisirCarte(carte);
  etat.disques = [];
  for (const d of disques) {
    const disque = await Backend.choisirDisque(d);
    if (disque) etat.disques.push(disque);
  }
  majSource();
  majDisques();
  journalPreparer();
  await decharger();
};
