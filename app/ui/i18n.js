/* Textes de l'interface, en français et en anglais. */
'use strict';

const TEXTES = {
  fr: {
    pret: 'Prêt à décharger',
    pret_detail: 'Choisir une carte et au moins un disque.',
    pret_un_disque: 'Une seule destination : la carte ne pourra pas être déclarée bonne à formater.',
    pret_ok: 'Tout est prêt.',
    onglet_decharger: 'Décharger', onglet_verifier: 'Vérifier un disque', onglet_historique: 'Historique', onglet_reglages: 'Réglages',
    carte: 'Carte', deposer: 'Déposer la carte ici', ou_choisir: 'ou cliquer pour choisir un dossier', changer: 'Changer',
    clips: 'clips', fichiers: 'fichiers', a_copier: 'à copier', format: 'format',
    destinations: 'Destinations', ajouter_disque: 'Ajouter un disque', sur: 'sur',
    nommage: 'Nommage', gabarit: 'Gabarit du dossier',
    gabarit_aide: 'Jetons : {carte} {date} {projet} {jour} {camera} {operateur}. « / » crée un sous-dossier.',
    projet: 'Projet', jour: 'Jour de tournage', camera: 'Caméra', operateur: 'Opérateur', lieu: 'Lieu',
    pourquoi_lent: 'Pourquoi c\'est lent', masquer: 'Masquer', mesure: 'Mesuré',
    hypotheses: 'Explications compatibles avec ces mesures — à vérifier',
    ouvrir_rapport: 'Ouvrir le rapport', ouvrir_dossier: 'Ouvrir le dossier', ejecter_carte: 'Éjecter la carte', nouvelle_copie: 'Nouvelle copie',
    file_attente: 'File d\'attente', fichier: 'Fichier', clip: 'Clip', taille: 'Taille', disques: 'Disques',
    journal_vide: 'Le journal s\'écrit ici pendant la copie : les problèmes d\'abord, puis les derniers fichiers vérifiés.',
    verification: 'Vérification', mode_complet: 'Complète', mode_carte: 'Complète + relecture de la carte', mode_taille: 'Taille seule (ne pas formater)',
    empreintes: 'Empreintes', mhl1: 'MHL v1.1 en plus', ejecter_fin: 'Éjecter la carte à la fin',
    annuler: 'Annuler', decharger: 'Décharger', mettre_en_file: 'Mettre en file',
    verifier_titre: 'Vérifier un disque',
    verifier_texte: 'Relit un déchargement à partir de ses manifestes (ASC MHL, ou MHL v1.1) et signale tout fichier manquant, modifié ou inattendu. Rien n\'est écrit sur le disque. Pour prouver que les données sont bien sur le support, éjectez le disque, rebranchez-le, puis vérifiez-le ici.',
    choisir_dossier_verifier: 'Choisir le dossier d\'un déchargement',
    historique_titre: 'Cartes déchargées sur ce poste',
    historique_texte: 'Le registre reste sur cet ordinateur. Il sert à reconnaître une carte déjà déchargée.',
    langue: 'Langue', langue_systeme: 'Celle du système', son_fin: 'Son à la fin de la copie', notif_fin: 'Notification à la fin de la copie',
    a_propos: 'À propos',
    avertissement_titre: 'Avant de commencer',
    avertissement_texte: 'SuperLoc Offload est fourni gratuitement, en l\'état, comme aide à la vérification de vos copies. Dans les limites permises par la loi ; vos droits de consommateur ne sont pas affectés.',
    avertissement_1: 'Gardez la carte intacte tant que deux copies vérifiées n\'existent pas sur deux disques différents.',
    avertissement_2: 'L\'outil ne formate jamais la carte et n\'y écrit rien.',
    avertissement_3: 'Ne fermez pas le capot pendant une copie : l\'ordinateur s\'endormirait.',
    compris: 'J\'ai compris',
    // États du bandeau
    inventaire: 'Inventaire de la carte…', controle: 'Contrôles avant copie…', copie: 'Copie et vérification',
    relecture_carte: 'Relecture de la carte', rapports: 'Écriture des manifestes et rapports…',
    refusee: 'Copie refusée', refusee_detail: 'Rien n\'a été écrit. Corriger puis relancer.',
    reste: 'reste', lecture: 'lecture', fichier_n: 'Fichier {i} sur {n}',
    en_panne: 'en panne', termine: 'terminé', en_cours: 'en cours', verification_en_cours: 'Vérification en cours',
    ecrit: 'écrit', relu: 'relu',
    deja_dechargee: 'Déjà déchargée le {date} ({verdict}) vers : {dest}',
    segments_manquants: 'Numérotation de segments interrompue sur la carte : {s}',
    illisibles: '{n} entrée(s) de la carte illisible(s) : ne pas formater.',
    pont_absent: 'Moteur injoignable',
    pont_absent_detail: 'L\'interface ne communique pas avec le moteur de copie. Réinstaller l\'application.',
    erreur: 'Erreur',
    libres: 'libres', necessaires: 'nécessaires', insuffisant: 'espace insuffisant',
    disque_absent: 'dossier introuvable — disque débranché ?',
    retirer: 'Retirer',
    copie_annulee: 'Copie annulée',
    attente: 'en attente',
    problemes: 'problème(s)', derniers: 'derniers fichiers vérifiés',
    verif_conforme: 'CONFORME', verif_non_conforme: 'NON CONFORME',
    verif_resume: '{ok} fichier(s) conformes sur {n} · {format}',
    manquant_ou_modifie: 'Problèmes', inattendus: 'Fichiers absents du manifeste',
    date: 'Date', carte_col: 'Carte', verdict: 'Verdict', taille_col: 'Taille',
    aucun_historique: 'Aucune carte déchargée sur ce poste pour l\'instant.',
    ejection_ok: 'Carte éjectée.', ejection_ko: 'Éjection impossible : {e}',
    copie_lancee_file: 'Ajoutée à la file d\'attente.',
    bon: 'BON À FORMATER', conforme: 'COPIE CONFORME — NE PAS FORMATER ENCORE', echec: 'ÉCHEC — NE PAS FORMATER', annule: 'ANNULÉ — NE PAS FORMATER',
    detail_bon: 'Deux copies au moins, relues depuis deux disques différents.',
    detail_conforme: 'Les copies sont identiques à la carte, mais il manque une condition pour formater.',
    detail_echec: 'Au moins un fichier n\'est pas correctement copié. Voir ci-dessous.',
    detail_annule: 'Les fichiers déjà vérifiés sont conservés ; les autres n\'ont pas été copiés.',
    niveau_support: 'relu depuis le disque', niveau_cache: 'identique, relecture peut-être servie par le cache', niveau_taille: 'taille seule', niveau_echec: 'échec',
    notif_titre: 'SuperLoc Offload',
    preparer_suivante: 'Préparer la carte suivante', masquer_preparation: 'Replier',
    file_en_pause: 'File en pause après un échec.', reprendre_file: 'Reprendre la file',
  },
  en: {
    pret: 'Ready to offload',
    pret_detail: 'Choose a card and at least one disk.',
    pret_un_disque: 'Only one destination: the card cannot be declared safe to format.',
    pret_ok: 'Ready.',
    onglet_decharger: 'Offload', onglet_verifier: 'Verify a disk', onglet_historique: 'History', onglet_reglages: 'Settings',
    carte: 'Card', deposer: 'Drop the card here', ou_choisir: 'or click to choose a folder', changer: 'Change',
    clips: 'clips', fichiers: 'files', a_copier: 'to copy', format: 'format',
    destinations: 'Destinations', ajouter_disque: 'Add a disk', sur: 'of',
    nommage: 'Naming', gabarit: 'Folder template',
    gabarit_aide: 'Tokens: {carte} {date} {projet} {jour} {camera} {operateur}. “/” creates a subfolder.',
    projet: 'Project', jour: 'Shooting day', camera: 'Camera', operateur: 'Operator', lieu: 'Location',
    pourquoi_lent: 'Why it is slow', masquer: 'Hide', mesure: 'Measured',
    hypotheses: 'Explanations consistent with these measurements — to be checked',
    ouvrir_rapport: 'Open report', ouvrir_dossier: 'Open folder', ejecter_carte: 'Eject card', nouvelle_copie: 'New offload',
    file_attente: 'Queue', fichier: 'File', clip: 'Clip', taille: 'Size', disques: 'Disks',
    journal_vide: 'The log appears here during the copy: problems first, then the latest verified files.',
    verification: 'Verification', mode_complet: 'Full', mode_carte: 'Full + card re-read', mode_taille: 'Size only (do not format)',
    empreintes: 'Checksums', mhl1: 'Also MHL v1.1', ejecter_fin: 'Eject card when done',
    annuler: 'Cancel', decharger: 'Offload', mettre_en_file: 'Add to queue',
    verifier_titre: 'Verify a disk',
    verifier_texte: 'Re-reads an offload from its manifests (ASC MHL, or MHL v1.1) and reports any missing, modified or unexpected file. Nothing is written to the disk. To prove the data is on the media, eject the disk, plug it back in, then verify it here.',
    choisir_dossier_verifier: 'Choose an offload folder',
    historique_titre: 'Cards offloaded on this computer',
    historique_texte: 'The register stays on this computer. It is used to recognise a card that was already offloaded.',
    langue: 'Language', langue_systeme: 'System language', son_fin: 'Sound when the copy ends', notif_fin: 'Notification when the copy ends',
    a_propos: 'About',
    avertissement_titre: 'Before you start',
    avertissement_texte: 'SuperLoc Offload is provided free of charge, as is, as an aid to verify your copies. To the extent permitted by law; your consumer rights are not affected.',
    avertissement_1: 'Keep the card untouched until two verified copies exist on two different disks.',
    avertissement_2: 'The tool never formats the card and never writes to it.',
    avertissement_3: 'Do not close the lid during a copy: the computer would go to sleep.',
    compris: 'I understand',
    inventaire: 'Scanning the card…', controle: 'Pre-copy checks…', copie: 'Copy and verification',
    relecture_carte: 'Re-reading the card', rapports: 'Writing manifests and reports…',
    refusee: 'Copy refused', refusee_detail: 'Nothing was written. Fix and start again.',
    reste: 'left', lecture: 'read', fichier_n: 'File {i} of {n}',
    en_panne: 'failed', termine: 'done', en_cours: 'in progress', verification_en_cours: 'Verifying',
    ecrit: 'written', relu: 'read back',
    deja_dechargee: 'Already offloaded on {date} ({verdict}) to: {dest}',
    segments_manquants: 'Segment numbering gap on the card: {s}',
    illisibles: '{n} unreadable card entr(ies): do not format.',
    pont_absent: 'Engine unreachable',
    pont_absent_detail: 'The interface cannot talk to the copy engine. Reinstall the application.',
    erreur: 'Error',
    libres: 'free', necessaires: 'needed', insuffisant: 'not enough space',
    disque_absent: 'folder not found — disk unplugged?',
    retirer: 'Remove',
    copie_annulee: 'Copy cancelled',
    attente: 'waiting',
    problemes: 'problem(s)', derniers: 'latest verified files',
    verif_conforme: 'VERIFIED', verif_non_conforme: 'NOT VERIFIED',
    verif_resume: '{ok} file(s) verified out of {n} · {format}',
    manquant_ou_modifie: 'Problems', inattendus: 'Files not listed in the manifest',
    date: 'Date', carte_col: 'Card', verdict: 'Verdict', taille_col: 'Size',
    aucun_historique: 'No card offloaded on this computer yet.',
    ejection_ok: 'Card ejected.', ejection_ko: 'Could not eject: {e}',
    copie_lancee_file: 'Added to the queue.',
    bon: 'SAFE TO FORMAT', conforme: 'COPY VERIFIED — DO NOT FORMAT YET', echec: 'FAILED — DO NOT FORMAT', annule: 'CANCELLED — DO NOT FORMAT',
    detail_bon: 'At least two copies, read back from two different disks.',
    detail_conforme: 'The copies match the card, but one condition for formatting is missing.',
    detail_echec: 'At least one file was not copied correctly. See below.',
    detail_annule: 'Files already verified are kept; the others were not copied.',
    niveau_support: 'read back from disk', niveau_cache: 'identical, read-back may have come from cache', niveau_taille: 'size only', niveau_echec: 'failed',
    notif_titre: 'SuperLoc Offload',
    preparer_suivante: 'Prepare the next card', masquer_preparation: 'Collapse',
    file_en_pause: 'Queue paused after a failure.', reprendre_file: 'Resume queue',
  },
};

let LANGUE = 'fr';

function choisirLangue(code) {
  const c = (code || navigator.language || 'fr').toLowerCase();
  LANGUE = c.startsWith('en') ? 'en' : 'fr';
  document.documentElement.lang = LANGUE;
  document.querySelectorAll('[data-t]').forEach((el) => {
    el.textContent = t(el.dataset.t);
  });
}

function t(cle, vars) {
  let s = (TEXTES[LANGUE] && TEXTES[LANGUE][cle]) ?? TEXTES.fr[cle] ?? cle;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.split(`{${k}}`).join(v);
  return s;
}

function octets(n) {
  const fr = LANGUE === 'fr';
  const u = fr ? ['octets', 'ko', 'Mo', 'Go', 'To'] : ['bytes', 'kB', 'MB', 'GB', 'TB'];
  if (!n || n < 1000) return `${n || 0} ${u[0]}`;
  let i = 0, v = n;
  while (v >= 1000 && i < 4) { v /= 1000; i++; }
  const s = v.toFixed(i >= 3 ? 2 : i === 2 ? 1 : 0);
  return `${fr ? s.replace('.', ',') : s} ${u[i]}`;
}

function debit(o) {
  if (!o || o <= 0) return '—';
  const mo = o / 1e6, fr = LANGUE === 'fr';
  const s = mo >= 1000 ? `${(mo / 1000).toFixed(2)} ${fr ? 'Go/s' : 'GB/s'}` : `${mo >= 100 ? mo.toFixed(0) : mo.toFixed(1)} ${fr ? 'Mo/s' : 'MB/s'}`;
  return fr ? s.replace('.', ',') : s;
}

function duree(sec) {
  sec = Math.max(0, Math.round(sec));
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  if (h) return `${h} h ${String(m).padStart(2, '0')} min`;
  if (m) return `${m} min ${String(s).padStart(2, '0')} s`;
  return `${s} s`;
}
