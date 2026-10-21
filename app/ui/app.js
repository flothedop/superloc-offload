/* SuperLoc Offload — interface.
 * Règle absolue : le vert et le rouge ne décorent jamais. Le vert ne vient
 * que d'un verdict « bon à formater » rendu par le moteur ; le rouge, que
 * d'un échec. */
'use strict';

const $ = (id) => document.getElementById(id);
const el = (tag, cls, texte) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (texte !== undefined) e.textContent = texte;
  return e;
};

const E = {
  reglages: null,
  source: null,
  apercus: [],
  volumes: [],
  tache: null,
  file: [],
  faits: [],
  pause: false,
  dernier: null,
  journal: { problemes: [], fichiers: new Map(), ok: 0 },
  rendu: false,
};

/* ------------------------------------------------------------- Bandeau */
function bandeau(etat, verdict, detail) {
  $('bandeau').dataset.etat = etat;
  $('verdict').textContent = verdict;
  $('detail').textContent = detail || '';
  const enCours = etat === 'copie';
  $('chiffre').classList.toggle('cache', !enCours);
  if (!enCours) { $('remplissage').style.width = '0'; $('reste').textContent = ''; }
}

function progression(fraction, reste) {
  const p = Math.max(0, Math.min(1, fraction));
  $('remplissage').style.width = `${(p * 100).toFixed(2)}%`;
  $('chiffre').firstChild.nodeValue = `${Math.floor(p * 100)}`;
  $('reste').textContent = reste ? `${t('reste')} ${duree(reste)}` : '';
}

/* ------------------------------------------------------------ Réglages */
let minuterieSauvegarde = null;
function sauver() {
  clearTimeout(minuterieSauvegarde);
  minuterieSauvegarde = setTimeout(() => BACKEND.reglagesEcrire(E.reglages).catch(() => {}), 300);
}

function appliquerReglages() {
  const r = E.reglages;
  $('gabarit').value = r.gabarit || '{date}_{carte}';
  $('projet').value = r.projet || '';
  $('jour').value = r.jour || '';
  $('camera').value = r.camera || '';
  $('operateur').value = r.operateur || '';
  $('mode').value = r.mode || 'complet';
  $('algos').value = (r.algos || ['xxh128', 'xxh64']).join(',');
  if (!$('algos').value) $('algos').value = 'xxh128,xxh64';
  $('mhl1').checked = r.mhl_v1 !== false;
  $('ejecter').checked = !!r.ejecter;
  $('r-langue').value = r.langue || '';
  $('r-operateur').value = r.operateur || '';
  $('r-email').value = r.email || '';
  $('r-lieu').value = r.lieu || '';
  $('r-son').checked = r.son !== false;
  $('r-notification').checked = r.notification !== false;
}

function lierReglages() {
  const champ = (id, cle, apres) => $(id).addEventListener('input', () => {
    E.reglages[cle] = $(id).value;
    sauver();
    if (apres) apres();
  });
  champ('gabarit', 'gabarit', majApercu);
  champ('projet', 'projet', majApercu);
  champ('jour', 'jour', majApercu);
  champ('camera', 'camera', majApercu);
  champ('operateur', 'operateur', () => { $('r-operateur').value = E.reglages.operateur; majApercu(); });
  champ('r-operateur', 'operateur', () => { $('operateur').value = E.reglages.operateur; });
  champ('r-email', 'email');
  champ('r-lieu', 'lieu');
  $('mode').addEventListener('change', () => { E.reglages.mode = $('mode').value; sauver(); });
  $('algos').addEventListener('change', () => { E.reglages.algos = $('algos').value.split(','); sauver(); });
  $('mhl1').addEventListener('change', () => { E.reglages.mhl_v1 = $('mhl1').checked; sauver(); });
  $('ejecter').addEventListener('change', () => { E.reglages.ejecter = $('ejecter').checked; sauver(); });
  $('r-son').addEventListener('change', () => { E.reglages.son = $('r-son').checked; sauver(); });
  $('r-notification').addEventListener('change', () => { E.reglages.notification = $('r-notification').checked; sauver(); });
  $('r-langue').addEventListener('change', () => {
    E.reglages.langue = $('r-langue').value;
    sauver();
    choisirLangue(E.reglages.langue);
    rafraichirTout();
  });
}

function valeursNommage() {
  const r = E.reglages;
  return { carte: E.source ? $('carte-nom').value.trim() || E.source.nom : 'CARTE', date: '', projet: r.projet || '', jour: r.jour || '', camera: r.camera || '', operateur: r.operateur || '' };
}

/* -------------------------------------------------------------- Onglets */
function montrerVue(nom) {
  document.querySelectorAll('.onglet').forEach((o) => o.classList.toggle('actif', o.dataset.vue === nom));
  document.querySelectorAll('.vue').forEach((v) => v.classList.toggle('cache', v.id !== `vue-${nom}`));
  if (nom === 'historique') afficherHistorique();
}

/* ---------------------------------------------------------------- Carte */
function estDestination(point) {
  return (E.reglages.actives || []).some((b) => b === point || b.startsWith(point.endsWith('/') || point.endsWith('\\') ? point : point + '/') || b.startsWith(point + '\\'));
}

function afficherTuiles() {
  const zone = $('tuiles');
  zone.textContent = '';
  const enCours = E.tache && E.tache.demande ? E.tache.demande.source : null;
  const cartes = E.volumes.filter((v) => (v.camera || v.amovible) && !estDestination(v.point_montage) && v.point_montage !== enCours);
  for (const v of cartes.slice(0, 6)) {
    const b = el('button', 'tuile');
    b.type = 'button';
    if (E.source && E.source.chemin === v.point_montage) b.classList.add('choisie');
    b.append(el('span', 'tuile__nom', v.nom || v.point_montage));
    b.append(el('span', 'tuile__info mono', `${v.camera || v.systeme_fichiers} · ${octets(v.total - v.libre)}`));
    b.addEventListener('click', () => choisirSource(v.point_montage));
    zone.append(b);
  }
}

async function choisirSource(chemin) {
  if (E.tache && !E.file) return;
  $('carte-vide').classList.add('cache');
  $('carte-pleine').classList.remove('cache');
  $('carte-nom').value = '…';
  $('carte-chemin').textContent = chemin;
  try {
    const s = await BACKEND.analyserSource(chemin);
    E.source = s;
    $('carte-nom').value = s.nom;
    $('carte-chemin').textContent = s.chemin;
    $('fait-clips').textContent = s.nb_clips;
    $('fait-fichiers').textContent = s.nb_fichiers;
    $('fait-taille').textContent = octets(s.octets);
    $('fait-camera').textContent = s.camera || (s.volume ? s.volume.systeme_fichiers : '—');
    $('carte-structure').textContent = s.volume ? `${s.volume.systeme_fichiers}` : '';
    const deja = (s.deja || []).slice(-1)[0];
    $('carte-deja').classList.toggle('cache', !deja);
    if (deja) {
      $('carte-deja').textContent = t('deja_dechargee', {
        date: new Date(deja.date).toLocaleString(LANGUE),
        verdict: texteVerdict(deja.verdict),
        dest: deja.destinations.join(', '),
      });
    }
    const alertes = [];
    if (s.illisibles && s.illisibles.length) alertes.push(t('illisibles', { n: s.illisibles.length }));
    if (s.segments_manquants && s.segments_manquants.length) alertes.push(t('segments_manquants', { s: s.segments_manquants.join(', ') }));
    $('carte-alerte').classList.toggle('cache', !alertes.length);
    $('carte-alerte').textContent = alertes.join(' ');
  } catch (e) {
    E.source = null;
    $('carte-nom').value = '';
    $('carte-alerte').classList.remove('cache');
    $('carte-alerte').textContent = String(e);
  }
  afficherTuiles();
  majApercu();
  majPret();
}

function oublierSource() {
  E.source = null;
  $('carte-vide').classList.remove('cache');
  $('carte-pleine').classList.add('cache');
  $('carte-structure').textContent = '';
  afficherTuiles();
  majApercu();
  majPret();
}

/* --------------------------------------------------------- Destinations */
let minuterieApercu = null;
function majApercu() {
  clearTimeout(minuterieApercu);
  minuterieApercu = setTimeout(async () => {
    const bases = E.reglages.destinations || [];
    try {
      E.apercus = await BACKEND.apercu({ bases, gabarit: E.reglages.gabarit || '', valeurs: valeursNommage() });
    } catch (_) {
      E.apercus = [];
    }
    afficherDisques();
  }, 120);
}

function afficherDisques() {
  const zone = $('disques');
  zone.textContent = '';
  const bases = E.reglages.destinations || [];
  const actives = new Set(E.reglages.actives || []);
  const besoin = E.source ? E.source.octets * 1.01 + 64e6 : 0;
  bases.forEach((b, i) => {
    const a = E.apercus.find((x) => x.base === b) || { base: b, dossier: b, volume: null, existe_base: true };
    const ligne = el('div', 'disque');
    const case_ = el('input');
    case_.type = 'checkbox';
    case_.checked = actives.has(b);
    case_.disabled = !!E.tache;
    case_.addEventListener('change', () => {
      const s = new Set(E.reglages.actives || []);
      if (case_.checked) s.add(b); else s.delete(b);
      E.reglages.actives = bases.filter((x) => s.has(x));
      sauver();
      afficherDisques();
      afficherTuiles();
      majPret();
    });
    ligne.append(case_);

    const centre = el('div', 'disque__centre');
    const v = a.volume;
    const nomBase = b.split(/[\\/]/).filter(Boolean).slice(-1)[0] || b;
    const nomVol = v && v.nom && v.nom !== '/' && !/^[A-Za-z]:\\?$/.test(v.nom) ? v.nom : '';
    const titre = el('div', 'disque__nom', nomVol ? (nomVol === nomBase ? nomVol : `${nomVol} — ${nomBase}`) : nomBase);
    if (v) titre.append(el('span', 'disque__fs mono', ` ${v.systeme_fichiers}`));
    centre.append(titre);
    centre.append(el('div', 'disque__chemin mono', actives.has(b) ? a.dossier : b));
    if (!a.existe_base) {
      centre.append(el('div', 'disque__espace insuffisant', t('disque_absent')));
    } else if (v && v.total) {
      const jauge = el('div', 'jauge');
      const occ = el('div', 'jauge__occupe');
      occ.style.width = `${((v.total - v.libre) / v.total) * 100}%`;
      const ajout = el('div', 'jauge__ajout');
      ajout.style.left = occ.style.width;
      ajout.style.width = `${Math.min(100, (besoin / v.total) * 100)}%`;
      jauge.append(occ, ajout);
      const manque = besoin > v.libre;
      if (manque) jauge.classList.add('jauge--pleine');
      centre.append(jauge);
      const esp = el('div', 'disque__espace', `${octets(v.libre)} ${t('libres')}${besoin ? ` · ${octets(besoin)} ${t('necessaires')}` : ''}${manque ? ` — ${t('insuffisant')}` : ''}`);
      if (manque) esp.classList.add('insuffisant');
      centre.append(esp);
    }
    ligne.append(centre);

    const retirer = el('button', 'disque__retirer', '×');
    retirer.type = 'button';
    retirer.title = t('retirer');
    retirer.disabled = !!E.tache;
    retirer.addEventListener('click', () => {
      E.reglages.destinations = bases.filter((x, j) => j !== i);
      E.reglages.actives = (E.reglages.actives || []).filter((x) => x !== b);
      sauver();
      majApercu();
      majPret();
    });
    ligne.append(retirer);
    zone.append(ligne);
  });
  const n = (E.reglages.actives || []).length;
  $('compte-disques').textContent = `${n} ${t('sur')} ${bases.length}`;
  const ac = (E.reglages.actives || [])[0];
  const ap = E.apercus.find((x) => x.base === ac);
  $('nommage-apercu').textContent = ap ? ap.dossier.split(/[\\/]/).slice(-1)[0] : '';
}

async function ajouterDisque(chemin) {
  const c = chemin || (await BACKEND.choisirDossier());
  if (!c) return;
  const d = E.reglages.destinations || [];
  if (!d.includes(c)) d.push(c);
  E.reglages.destinations = d;
  const a = new Set(E.reglages.actives || []);
  a.add(c);
  E.reglages.actives = d.filter((x) => a.has(x));
  sauver();
  majApercu();
  afficherTuiles();
  majPret();
}

/* -------------------------------------------------------------- Lancer */
function demandeCourante() {
  const r = E.reglages;
  return {
    source: E.source.chemin,
    nom: $('carte-nom').value.trim() || E.source.nom,
    bases: (r.actives || []).slice(),
    gabarit: r.gabarit || '',
    valeurs: valeursNommage(),
    mode: r.mode || 'complet',
    algos: (r.algos && r.algos.length) ? r.algos : ['xxh128', 'xxh64'],
    mhl_v1: r.mhl_v1 !== false,
    email: r.email || '',
    lieu: r.lieu || '',
    commentaire: '',
    langue: LANGUE,
    ejecter: !!r.ejecter,
    empreinte: E.source.empreinte || null,
    octets: E.source.octets,
  };
}

function majPret() {
  const n = (E.reglages.actives || []).length;
  const memeCarte = E.tache && E.tache.demande && E.source && E.tache.demande.source === E.source.chemin;
  const ok = !!E.source && !memeCarte && n > 0 && E.apercus.filter((a) => (E.reglages.actives || []).includes(a.base)).every((a) => a.existe_base);
  $('btn-decharger').disabled = !ok;
  $('btn-decharger').textContent = E.tache ? t('mettre_en_file') : t('decharger');
  if (E.tache || E.dernier) return;
  if (!ok) bandeau('repos', t('pret'), t('pret_detail'));
  else bandeau('repos', t('pret'), n < 2 ? t('pret_un_disque') : t('pret_ok'));
}

function surDecharger() {
  if (!E.source) return;
  const d = demandeCourante();
  if (E.tache) {
    E.file.push(d);
    afficherFile();
    $('pied-info').textContent = t('copie_lancee_file');
    oublierSource();
    return;
  }
  E.faits = [];
  E.pause = false;
  lancer(d);
}

function afficherFile() {
  const liste = $('file-liste');
  liste.textContent = '';
  const visible = E.file.length > 0 || E.faits.length > 1 || E.pause;
  $('file').classList.toggle('cache', !visible);
  $('file-compte').classList.toggle('cache', !E.file.length);
  $('file-compte').textContent = E.file.length ? `+${E.file.length}` : '';
  // Cartes déjà traitées dans cette série : un échec reste visible même
  // quand la carte suivante a réussi.
  for (const f of E.faits) {
    const li = el('li', `file__fait verdict-${f.verdict}`);
    li.append(el('b', null, f.nom), document.createTextNode(` — ${texteVerdict(f.verdict)}`));
    liste.append(li);
  }
  for (const d of E.file) {
    liste.append(el('li', null, `${d.nom} — ${octets(d.octets)} → ${d.bases.length} ${t('destinations').toLowerCase()} · ${t('attente')}`));
  }
  if (E.pause && E.file.length) {
    const li = el('li', 'file__pause');
    li.append(document.createTextNode(t('file_en_pause') + ' '));
    const b = el('button', 'lien', t('reprendre_file'));
    b.type = 'button';
    b.addEventListener('click', () => {
      E.pause = false;
      const suivante = E.file.shift();
      afficherFile();
      if (suivante) lancer(suivante);
    });
    li.append(b);
    liste.append(li);
  }
}

function reinitialiserJournal(noms) {
  E.journal = { problemes: [], fichiers: new Map(), ok: 0 };
  const corps = $('journal-corps');
  corps.textContent = '';
  corps.append($('journal-vide') || el('p', 'journal__vide', t('journal_vide')));
  const entete = $('entete-dest');
  entete.textContent = t('disques');
  noms.forEach((_, i) => entete.append(el('span', 'repere', String(i + 1))));
  document.documentElement.style.setProperty('--colonnes-dest', `${Math.max(60, noms.length * 22 + 50)}px`);
}

function compacter(oui, texte) {
  const vue = $('vue-decharger');
  vue.classList.toggle('compacte', oui);
  if (texte !== undefined) {
    const r = $('resume-texte');
    r.textContent = '';
    r.append(el('b', null, texte.nom), document.createTextNode(` → ${texte.vers}`));
  }
  $('resume').classList.toggle('cache', !E.tache && !E.dernier);
  $('resume-basculer').textContent = oui ? t('preparer_suivante') : t('masquer_preparation');
}

async function lancer(demande) {
  E.tache = { demande, noms: [], volumes: [], phase: 'inventaire', total: demande.octets, fin: false };
  E.dernier = null;
  $('resultat').classList.add('cache');
  $('analyse').classList.add('cache');
  $('btn-annuler').classList.remove('cache');
  $('ajouter').disabled = true;
  bandeau('copie', t('inventaire'), demande.nom);
  progression(0);
  compacter(true, { nom: demande.nom, vers: demande.bases.join(' · ') });
  reinitialiserJournal(demande.bases);
  preparerDebits(demande.bases.map((b) => b.split(/[\\/]/).filter(Boolean).slice(-2).join('/')));
  afficherDisques();
  afficherTuiles();
  majPret();

  let issue;
  try {
    issue = await BACKEND.decharger(demande);
  } catch (e) {
    issue = { issue: 'refus', erreurs: [{ message: String(e), action: '' }] };
  }
  E.tache = null;
  $('btn-annuler').classList.add('cache');
  $('ajouter').disabled = false;
  afficherIssue(demande, issue);
  const verdict = issue.issue === 'refus' ? 'echec' : issue.rapport.verdict;
  E.faits.push({ nom: demande.nom, verdict });
  // Un échec arrête la file : il doit être lu avant de passer à la suite.
  if (verdict === 'echec' || verdict === 'annule') E.pause = true;
  majApercu();
  afficherDisques();
  afficherTuiles();
  majPret();
  afficherFile();
  if (E.file.length && !E.pause) {
    const suivante = E.file.shift();
    afficherFile();
    setTimeout(() => lancer(suivante), 800);
  }
}

/* ---------------------------------------------------- Événements moteur */
function surEvenement(e) {
  const tache = E.tache;
  if (!tache) return;
  switch (e.type) {
    case 'phase':
      tache.phase = e.phase;
      if (e.phase === 'controle') bandeau('copie', t('controle'), tache.demande.nom);
      if (e.phase === 'copie') bandeau('copie', `${t('copie')} — ${tache.demande.nom}`, '');
      if (e.phase === 'relecture-carte') bandeau('copie', t('relecture_carte'), tache.demande.nom);
      if (e.phase === 'rapports') bandeau('copie', t('rapports'), tache.demande.nom);
      break;
    case 'debut':
      tache.total = e.octets;
      tache.nb = e.nb_fichiers;
      tache.volumes = e.volumes || [];
      tache.noms = (e.volumes || []).map((v, i) => (v && v.nom && v.nom !== '/' ? v.nom : tache.demande.bases[i].split(/[\\/]/).filter(Boolean).slice(-1)[0]));
      preparerDebits(tache.noms);
      compacter($('vue-decharger').classList.contains('compacte'), { nom: tache.demande.nom, vers: tache.noms.join(' · ') });
      break;
    case 'avancement':
      surAvancement(e);
      break;
    case 'copie': {
      if (e.niveau === 'echec') {
        E.journal.problemes.push({ chemin: e.chemin, destination: e.destination, message: e.message || (e.erreur && e.erreur.code) || '', action: e.action || '' });
      } else {
        E.journal.ok++;
      }
      const j = E.journal.fichiers;
      const f = j.get(e.index) || { chemin: e.chemin, etats: {} };
      f.etats[e.destination] = e.niveau;
      j.delete(e.index);
      j.set(e.index, f);
      if (j.size > 200) j.delete(j.keys().next().value);
      demanderRendu();
      break;
    }
    case 'source':
      E.journal.problemes.push({ chemin: e.chemin, destination: null, message: e.message || '', action: e.action || '' });
      demanderRendu();
      break;
    case 'diagnostic':
      afficherDiagnostic(e);
      break;
    case 'destination':
      if (e.erreur) {
        E.journal.problemes.push({ chemin: tache.noms[e.destination] || `#${e.destination + 1}`, destination: e.destination, message: e.message || '', action: e.action || '' });
        demanderRendu();
      }
      break;
    default:
  }
}

function surAvancement(a) {
  const tache = E.tache;
  const passes = tache.demande.mode === 'taille' ? 1 : 2;
  const total = Math.max(a.total, 1);
  let f = 1;
  a.destinations.forEach((d) => {
    if (d.en_panne) return;
    const x = d.termine ? 1 : (d.ecrits + d.relus) / (passes * total);
    f = Math.min(f, x);
  });
  if (a.relecture_carte != null) f = a.relecture_carte / total;
  progression(f, a.restant_s);
  if (tache.phase === 'copie') {
    $('detail').textContent = `${t('fichier_n', { i: a.index + 1, n: a.nb_fichiers })} · ${a.chemin} · ${t('lecture')} ${debit(a.debit_lecture)}`;
  }
  a.destinations.forEach((d, i) => majDebit(i, d, total, passes));
}

/* ----------------------------------------------------------- Débits */
function preparerDebits(noms) {
  const zone = $('debits');
  zone.textContent = '';
  zone.classList.remove('cache');
  noms.forEach((nom, i) => {
    const l = el('div', 'debit');
    l.id = `debit-${i}`;
    l.append(el('span', 'repere', String(i + 1)));
    l.append(el('span', 'debit__nom', nom));
    const barre = el('div', 'debit__barre');
    const ecrit = el('div', 'debit__ecrit');
    const relu = el('div', 'debit__relu');
    barre.append(ecrit, relu);
    l.append(barre);
    l.append(el('span', 'debit__valeur', '—'));
    l.append(el('span', 'debit__etat mono', ''));
    zone.append(l);
  });
}

function majDebit(i, d, total, passes) {
  const l = $(`debit-${i}`);
  if (!l) return;
  l.querySelector('.debit__ecrit').style.width = `${Math.min(100, (d.ecrits / total) * 100)}%`;
  l.querySelector('.debit__relu').style.width = passes > 1 ? `${Math.min(100, (d.relus / total) * 100)}%` : '0';
  l.querySelector('.debit__valeur').textContent = d.termine ? '' : debit(d.debit);
  const etat = d.en_panne ? t('en_panne') : d.termine ? t('termine') : `${d.fichiers_ok} ✓${d.fichiers_echec ? ` · ${d.fichiers_echec} ✗` : ''}`;
  l.querySelector('.debit__etat').textContent = etat;
  l.classList.toggle('debit--panne', d.en_panne);
}

/* ---------------------------------------------------------- Journal */
function demanderRendu() {
  if (E.rendu) return;
  E.rendu = true;
  requestAnimationFrame(() => {
    E.rendu = false;
    rendreJournal();
  });
}

function ligneJournal(entree, echec) {
  const l = el('div', echec ? 'ligne ligne--echec' : 'ligne');
  const morceaux = entree.chemin.split('/');
  l.append(el('div', 'ligne__fichier', morceaux.slice(-1)[0]));
  l.append(el('div', 'ligne__clip', morceaux.length > 1 ? morceaux.slice(-2, -1)[0] : ''));
  const etats = el('div', 'ligne__etats');
  const nb = E.tache ? E.tache.demande.bases.length : (E.dernier ? E.dernier.demande.bases.length : 1);
  for (let i = 0; i < nb; i++) {
    const p = el('span', 'pastille');
    let niveau = entree.etats ? entree.etats[i] : (entree.destination === i || entree.destination == null ? 'echec' : undefined);
    if (niveau === undefined) {
      p.dataset.etat = 'attente';
    } else {
      p.dataset.etat = niveau === 'echec' ? 'echec' : niveau === 'support' ? 'ok' : 'approx';
      p.textContent = niveau === 'echec' ? '✗' : niveau === 'support' ? '✓' : '≈';
      p.title = t({ echec: 'niveau_echec', support: 'niveau_support', 'cache-possible': 'niveau_cache', taille: 'niveau_taille' }[niveau] || 'niveau_echec');
    }
    etats.append(p);
  }
  l.append(etats);
  if (echec) {
    const noms = E.tache ? E.tache.noms : (E.noms || []);
    const qui = entree.destination != null ? `${noms[entree.destination] || '#' + (entree.destination + 1)} : ` : '';
    l.append(el('div', 'ligne__motif', `${qui}${entree.message}${entree.action ? ` — ${entree.action}` : ''}`));
  }
  return l;
}

function rendreJournal() {
  const corps = $('journal-corps');
  const j = E.journal;
  corps.textContent = '';
  if (!j.problemes.length && !j.fichiers.size) {
    corps.append(el('p', 'journal__vide', t('journal_vide')));
    $('journal-pied').classList.add('cache');
    return;
  }
  for (const p of j.problemes.slice(0, 300)) corps.append(ligneJournal(p, true));
  const recents = Array.from(j.fichiers.values()).slice(-60).reverse();
  for (const r of recents) {
    if (Object.values(r.etats).includes('echec')) continue;
    corps.append(ligneJournal(r, false));
  }
  $('journal-pied').classList.remove('cache');
  $('journal-pied').textContent = `${j.problemes.length} ${t('problemes')} · ${j.ok} ✓ · ${t('derniers')}`;
}

/* ------------------------------------------------------- Diagnostic */
function afficherDiagnostic(d) {
  if (!d || ((!d.faits || !d.faits.length) && (!d.hypotheses || !d.hypotheses.length))) return;
  const faits = $('analyse-faits');
  const hypos = $('analyse-hypotheses');
  faits.textContent = '';
  hypos.textContent = '';
  (d.faits || []).forEach((f) => faits.append(el('li', null, f)));
  (d.hypotheses || []).forEach((h) => {
    const li = el('li');
    li.append(el('span', 'hypo__titre', h.titre));
    li.append(document.createTextNode(h.parce_que));
    li.append(el('span', 'hypo__test', h.verifier));
    hypos.append(li);
  });
  if (d.lent || (d.hypotheses && d.hypotheses.length)) $('analyse').classList.remove('cache');
}

/* ------------------------------------------------------------ Issue */
function texteVerdict(v) {
  return { 'bon-a-formater': t('bon'), 'conforme-ne-pas-formater': t('conforme'), echec: t('echec'), annule: t('annule') }[v] || v;
}

function afficherIssue(demande, issue) {
  E.dernier = { demande, issue };
  $('debits').classList.remove('cache');
  const res = $('resultat');
  const rem = $('remarques');
  rem.textContent = '';
  res.classList.remove('cache');

  if (issue.issue === 'refus') {
    bandeau('alerte', t('refusee'), t('refusee_detail'));
    $('debits').classList.add('cache');
    for (const e of issue.erreurs) {
      const li = el('li', 'remarque--alerte');
      li.append(el('b', null, e.message));
      if (e.action) li.append(el('span', 'remarque__action', e.action));
      rem.append(li);
    }
    ['btn-rapport', 'btn-dossier', 'btn-ejecter'].forEach((id) => $(id).classList.add('cache'));
    son('echec');
    return;
  }

  const r = issue.rapport;
  E.noms = issue.noms || [];
  const etat = { 'bon-a-formater': 'conforme', 'conforme-ne-pas-formater': 'attention', echec: 'alerte', annule: 'alerte' }[r.verdict];
  const detail = { 'bon-a-formater': t('detail_bon'), 'conforme-ne-pas-formater': t('detail_conforme'), echec: t('detail_echec'), annule: t('detail_annule') }[r.verdict];
  const duree_s = (new Date(r.fin) - new Date(r.debut)) / 1000;
  bandeau(etat, texteVerdict(r.verdict), `${demande.nom} · ${r.nb_fichiers} ${t('fichiers')} · ${octets(r.octets)} · ${duree(duree_s)} — ${detail}`);
  for (const x of issue.remarques || []) rem.append(el('li', null, x));
  if (issue.ejection) {
    rem.append(el('li', null, issue.ejection.Ok !== undefined || issue.ejection === null ? t('ejection_ok') : t('ejection_ko', { e: issue.ejection.Err })));
  }
  // Le journal final reprend tous les problèmes du rapport (avec action).
  if (issue.problemes && issue.problemes.length) {
    E.journal.problemes = issue.problemes;
    rendreJournal();
  }
  const premiere = r.destinations.find((d) => !d.erreur_fatale) || r.destinations[0];
  E.dernier.dossier = premiere ? premiere.dossier : null;
  ['btn-rapport', 'btn-dossier'].forEach((id) => $(id).classList.toggle('cache', !E.dernier.dossier));
  $('btn-ejecter').classList.toggle('cache', r.verdict === 'echec' || r.annule);
  r.destinations.forEach((d, i) => {
    const l = $(`debit-${i}`);
    if (!l) return;
    if (!d.erreur_fatale) {
      l.querySelector('.debit__ecrit').style.width = '100%';
      if (demande.mode !== 'taille') l.querySelector('.debit__relu').style.width = '100%';
    }
    l.classList.toggle('debit--panne', !!d.erreur_fatale);
    l.querySelector('.debit__valeur').textContent = debit(d.profil && d.profil.debit_moyen);
    l.querySelector('.debit__etat').textContent = d.erreur_fatale ? t('en_panne') : `${d.fichiers_ok}/${r.nb_fichiers} ✓`;
  });
  son({ 'bon-a-formater': 'bon', 'conforme-ne-pas-formater': 'neutre' }[r.verdict] || 'echec');
  if (E.reglages.notification !== false) BACKEND.notifier(t('notif_titre'), `${demande.nom} : ${texteVerdict(r.verdict)}`);
}

function nouvelleCopie() {
  E.dernier = null;
  compacter(false);
  $('resultat').classList.add('cache');
  $('debits').classList.add('cache');
  $('analyse').classList.add('cache');
  reinitialiserJournal([]);
  rendreJournal();
  oublierSource();
}

/* -------------------------------------------------------------- Son */
/* Trois sons distincts : seul « bon à formater » a le son de la réussite ;
 * « ne pas formater encore » a une note neutre ; l'échec, un signal grave. */
function son(type) {
  if (E.reglages && E.reglages.son === false) return;
  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const notes = {
      bon: [[660, 0, 'sine'], [880, 0.16, 'sine']],
      neutre: [[520, 0, 'triangle']],
      echec: [[220, 0, 'square'], [220, 0.28, 'square'], [220, 0.56, 'square']],
    }[type] || [];
    for (const [f, d, forme] of notes) {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = forme;
      o.frequency.value = f;
      g.gain.setValueAtTime(0.0001, ctx.currentTime + d);
      g.gain.exponentialRampToValueAtTime(0.18, ctx.currentTime + d + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + d + 0.22);
      o.connect(g).connect(ctx.destination);
      o.start(ctx.currentTime + d);
      o.stop(ctx.currentTime + d + 0.25);
    }
  } catch (_) { /* pas de son : pas grave */ }
}

/* --------------------------------------------------------- Vérifier */
async function verifierUnDossier() {
  if (E.tache) return;
  const chemin = await BACKEND.choisirDossier();
  if (!chemin) return;
  const zone = $('verif');
  zone.classList.remove('cache');
  $('verif-chemin').textContent = chemin;
  $('verif-resume').className = 'verif__resume';
  $('verif-resume').textContent = t('verification_en_cours');
  $('verif-liste').textContent = '';
  $('btn-verifier').disabled = true;
  $('btn-verifier-annuler').classList.remove('cache');
  E.tache = { verification: true, demande: { bases: [] }, noms: [] };
  bandeau('copie', t('verification_en_cours'), chemin);
  try {
    const b = await BACKEND.verifierDossier(chemin, LANGUE);
    $('verif-resume').classList.add(b.conforme ? 'verif__resume--ok' : 'verif__resume--ko');
    $('verif-resume').textContent = `${b.conforme ? t('verif_conforme') : t('verif_non_conforme')} — ${t('verif_resume', { ok: b.conformes, n: b.nb_fichiers, format: b.format })}`;
    for (const [c, m] of b.problemes_texte || []) {
      const li = el('li', 'remarque--alerte');
      li.append(el('b', 'mono', c));
      li.append(el('span', 'remarque__action', m));
      $('verif-liste').append(li);
    }
    for (const x of b.inattendus || []) {
      const li = el('li');
      li.append(el('b', 'mono', x));
      li.append(el('span', 'remarque__action', t('inattendus')));
      $('verif-liste').append(li);
    }
    // Une vérification de dossier n'autorise pas à formater : jamais de vert
    // plein ici, seulement le résultat écrit.
    bandeau(b.conforme ? 'repos' : 'alerte', b.conforme ? t('verif_conforme') : t('verif_non_conforme'), chemin);
    son(b.conforme ? 'neutre' : 'echec');
  } catch (e) {
    $('verif-resume').classList.add('verif__resume--ko');
    $('verif-resume').textContent = String(e);
    bandeau('alerte', t('erreur'), String(e).split('\n')[0]);
  }
  E.tache = null;
  E.dernier = { verification: true, demande: { bases: [] } };
  $('btn-verifier').disabled = false;
  $('btn-verifier-annuler').classList.add('cache');
}

/* ------------------------------------------------------- Historique */
async function afficherHistorique() {
  const tab = $('historique');
  tab.textContent = '';
  let liste = [];
  try { liste = await BACKEND.registre(); } catch (_) { /* vide */ }
  if (!liste.length) {
    const tr = el('tr');
    tr.append(el('td', 'vide', t('aucun_historique')));
    tab.append(tr);
    return;
  }
  const tete = el('tr');
  [t('date'), t('carte_col'), t('fichiers'), t('taille_col'), t('verdict'), t('destinations')].forEach((x) => tete.append(el('th', null, x)));
  tab.append(tete);
  for (const e of liste) {
    const tr = el('tr');
    tr.append(el('td', 'mono', new Date(e.date).toLocaleString(LANGUE)));
    tr.append(el('td', null, e.carte));
    tr.append(el('td', 'n', String(e.nb_fichiers)));
    tr.append(el('td', 'n', octets(e.octets)));
    const v = el('td', `verdict-${e.verdict}`, texteVerdict(e.verdict));
    tr.append(v);
    tr.append(el('td', 'mono petit', e.destinations.join('\n')));
    tab.append(tr);
  }
}

/* ------------------------------------------------------- Démarrage */
function rafraichirTout() {
  afficherDisques();
  afficherTuiles();
  afficherFile();
  rendreJournal();
  if (!E.tache && !E.dernier) majPret();
}

async function suivreVolumes() {
  try {
    E.volumes = await BACKEND.volumes();
    afficherTuiles();
  } catch (_) { /* pas de liste : on réessaie */ }
  setTimeout(suivreVolumes, 2500);
}

async function demarrer() {
  if (!BACKEND) {
    choisirLangue();
    bandeau('alerte', t('pont_absent'), t('pont_absent_detail'));
    document.querySelectorAll('button, input, select').forEach((b) => { b.disabled = true; });
    return;
  }
  E.reglages = await BACKEND.reglagesLire();
  choisirLangue(E.reglages.langue);
  appliquerReglages();
  lierReglages();
  try {
    const i = await BACKEND.infos();
    $('version').textContent = `v${i.version}${BACKEND.reel ? '' : ' · démo'}`;
    $('a-propos').textContent = `SuperLoc Offload ${i.version} — ${i.os}. Gratuit, sans compte, sans connexion : aucune donnée ne quitte cet ordinateur.`;
  } catch (_) { /* rien */ }

  document.querySelectorAll('.onglet').forEach((o) => o.addEventListener('click', () => montrerVue(o.dataset.vue)));
  $('carte-vide').addEventListener('click', async () => { const c = await BACKEND.choisirDossier(); if (c) choisirSource(c); });
  $('carte-changer').addEventListener('click', oublierSource);
  $('carte-nom').addEventListener('input', majApercu);
  $('ajouter').addEventListener('click', () => ajouterDisque());
  $('btn-decharger').addEventListener('click', surDecharger);
  $('btn-annuler').addEventListener('click', () => BACKEND.annuler());
  $('btn-verifier').addEventListener('click', verifierUnDossier);
  $('btn-verifier-annuler').addEventListener('click', () => BACKEND.annuler());
  $('analyse-fermer').addEventListener('click', () => $('analyse').classList.add('cache'));
  $('btn-nouvelle').addEventListener('click', nouvelleCopie);
  $('resume-basculer').addEventListener('click', () => compacter(!$('vue-decharger').classList.contains('compacte')));
  $('btn-rapport').addEventListener('click', () => E.dernier && E.dernier.dossier && BACKEND.ouvrir(`${E.dernier.dossier}${E.dernier.dossier.includes('\\') ? '\\' : '/'}_offload${E.dernier.dossier.includes('\\') ? '\\' : '/'}rapport.html`));
  $('btn-dossier').addEventListener('click', () => E.dernier && E.dernier.dossier && BACKEND.ouvrir(E.dernier.dossier));
  $('btn-ejecter').addEventListener('click', async () => {
    if (!E.dernier || !E.dernier.demande.source) return;
    try { await BACKEND.ejecter(E.dernier.demande.source); $('pied-info').textContent = t('ejection_ok'); } catch (e) { $('pied-info').textContent = t('ejection_ko', { e: String(e) }); }
  });

  await BACKEND.ecouter('offload://evenement', surEvenement);
  await BACKEND.ecouter('offload://verification', (p) => {
    if (E.tache && E.tache.verification) {
      progression(p.total ? p.octets / p.total : p.index / Math.max(1, p.nb_fichiers));
      $('detail').textContent = `${t('fichier_n', { i: p.index + 1, n: p.nb_fichiers })} · ${p.chemin}`;
    }
  });
  await BACKEND.surDepot((chemins) => {
    if (!chemins.length || E.tache) return;
    if (!E.source) choisirSource(chemins[0]);
    else chemins.forEach((c) => ajouterDisque(c));
  });

  if (!E.reglages.avertissement_accepte) {
    const d = $('dialogue-avertissement');
    d.showModal();
    $('btn-accepter').addEventListener('click', () => {
      E.reglages.avertissement_accepte = true;
      sauver();
      d.close();
    });
  }

  majApercu();
  rafraichirTout();
  suivreVolumes();

  PRET.ouvrir();
}

// Essai automatique de bout en bout (OFFLOAD_AUTO), réservé aux tests.
// Défini d'emblée : l'appel peut arriver avant la fin du démarrage.
const PRET = (() => {
  let ouvrir;
  const p = new Promise((r) => { ouvrir = r; });
  p.ouvrir = ouvrir;
  return p;
})();
window.__essai = async (carte, disques) => {
    await PRET;
    E.reglages.destinations = disques;
    E.reglages.actives = disques.slice();
    await choisirSource(carte);
    E.apercus = await BACKEND.apercu({ bases: disques, gabarit: E.reglages.gabarit || '', valeurs: valeursNommage() });
    lancer(demandeCourante());
};

demarrer();
