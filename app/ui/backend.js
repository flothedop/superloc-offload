/* Pont vers le moteur.
 *
 * Dans l'application, tout passe par Tauri. Hors de l'application (aperçu
 * dans un navigateur), le simulateur n'est activé QUE si l'adresse contient
 * « ?demo » : sinon l'interface affiche une erreur franche. Un simulateur
 * qui répondrait « conforme » en silence serait la pire panne possible. */
'use strict';

const BACKEND = (() => {
  const tauri = window.__TAURI__;
  if (tauri && tauri.core) {
    const inv = (cmd, args) => tauri.core.invoke(cmd, args || {});
    return {
      reel: true,
      reglagesLire: () => inv('reglages_lire'),
      reglagesEcrire: (r) => inv('reglages_ecrire', { reglages: r }),
      volumes: () => inv('volumes'),
      analyserSource: (chemin) => inv('analyser_source', { chemin }),
      apercu: (demande) => inv('apercu', { demande }),
      choisirDossier: () => inv('choisir_dossier'),
      decharger: (demande) => inv('decharger', { demande }),
      annuler: () => inv('annuler'),
      verifierDossier: (chemin, langue) => inv('verifier_dossier', { chemin, langue }),
      registre: () => inv('registre_lire'),
      ouvrir: (chemin) => inv('ouvrir', { chemin }),
      ejecter: (chemin) => inv('ejecter', { chemin }),
      infos: () => inv('infos'),
      ecouter: (nom, f) => tauri.event.listen(nom, (e) => f(e.payload)),
      surDepot: (f) => tauri.event.listen('tauri://drag-drop', (e) => f((e.payload && e.payload.paths) || [])),
      notifier: async (titre, corps) => {
        try {
          const n = tauri.notification;
          if (n) {
            let ok = await n.isPermissionGranted();
            if (!ok) ok = (await n.requestPermission()) === 'granted';
            if (ok) n.sendNotification({ title: titre, body: corps });
            return;
          }
          await inv('plugin:notification|notify', { options: { title: titre, body: corps } });
        } catch (_) { /* une notification manquée n'est pas grave */ }
      },
    };
  }

  if (!new URLSearchParams(location.search).has('demo')) return null;
  return simulateur();
})();

/* ------------------------------------------------------------ Simulateur */
function simulateur() {
  const ecouteurs = {};
  const emettre = (nom, p) => (ecouteurs[nom] || []).forEach((f) => f(p));
  let annule = false;
  let reglages = { langue: '', operateur: 'Flo', email: '', projet: 'Pub Lille', jour: '2', camera: 'A', lieu: '',
    gabarit: '{date}_{carte}', destinations: ['/Volumes/SSD_PROD/Rushs', '/Volumes/BACKUP/Rushs'],
    actives: ['/Volumes/SSD_PROD/Rushs', '/Volumes/BACKUP/Rushs'], mode: 'complet', algos: ['xxh128', 'xxh64'],
    mhl_v1: true, son: false, notification: false, ejecter: false, avertissement_accepte: true };
  const vol = (nom, fs, total, libre, amovible, disque) => ({ point_montage: `/Volumes/${nom}`, nom, systeme_fichiers: fs, amovible, reseau: false, total, libre, id_volume: nom, id_disque: disque });
  const volumes = [
    { ...vol('A001', 'exfat', 512e9, 60e9, true, 'disk4'), camera: 'RED' },
    { ...vol('SSD_PROD', 'apfs', 2e12, 1.2e12, true, 'disk5'), camera: null },
    { ...vol('BACKUP', 'exfat', 4e12, 2.9e12, true, 'disk6'), camera: null },
  ];
  const fichiers = [];
  for (let c = 1; c <= 6; c++) for (let s = 1; s <= 3; s++) fichiers.push({ chemin: `A001_1021XY.RDM/A001_C00${c}_1021XY.RDC/A001_C00${c}_1021XY_00${s}.R3D`, taille: 4.1e9 });
  const mode = new URLSearchParams(location.search).get('demo');
  const total = fichiers.reduce((a, f) => a + f.taille, 0);
  const attendre = (ms) => new Promise((r) => setTimeout(r, ms));

  return {
    reel: false,
    reglagesLire: async () => reglages,
    reglagesEcrire: async (r) => { reglages = r; },
    volumes: async () => volumes,
    analyserSource: async (chemin) => ({ chemin, nom: 'A001', nb_fichiers: fichiers.length, octets: total, nb_clips: 6, dossiers_vides: 0, camera: 'RED', volume: volumes[0], exclus: 3, illisibles: [], segments_manquants: [], deja: mode === 'deja' ? [{ date: '2026-10-20T18:04:00Z', carte: 'A001', verdict: 'bon-a-formater', destinations: ['/Volumes/SSD_PROD/Rushs/2026-10-20_A001'] }] : [] }),
    apercu: async (d) => d.bases.map((b) => ({ base: b, dossier: `${b}/2026-10-22_A001`, volume: volumes.find((v) => b.startsWith(v.point_montage)) || null, existe_base: true })),
    choisirDossier: async () => '/Volumes/RAID/Rushs',
    decharger: async (demande) => {
      annule = false;
      const n = demande.bases.length;
      emettre('offload://evenement', { type: 'phase', phase: 'inventaire' });
      await attendre(300);
      emettre('offload://evenement', { type: 'debut', nom: demande.nom, nb_fichiers: fichiers.length, octets: total, nb_clips: 6, destinations: demande.bases, volumes: demande.bases.map((b) => volumes.find((v) => b.startsWith(v.point_montage))) });
      emettre('offload://evenement', { type: 'phase', phase: 'copie' });
      const debits = [1450e6, 380e6, 940e6];
      const etapes = mode === 'lent' ? 60 : 40;
      for (let k = 1; k <= etapes && !annule; k++) {
        await attendre(70);
        const f = k / etapes;
        const index = Math.min(fichiers.length - 1, Math.floor(f * fichiers.length));
        emettre('offload://evenement', { type: 'avancement', lus: total * Math.min(1, f * 1.15), total, debit_lecture: 1.4e9, index, nb_fichiers: fichiers.length, chemin: fichiers[index].chemin, ecoule_s: k * 9, restant_s: (etapes - k) * 9,
          destinations: Array.from({ length: n }, (_, i) => ({ ecrits: total * Math.min(1, f * (i ? 0.9 : 1.1)), relus: total * Math.max(0, f * 0.95 - 0.05), debit: mode === 'lent' && i === 1 ? 36e6 : debits[i % 3], fichiers_ok: index, fichiers_echec: 0, en_panne: false, termine: false })) });
        if (k % 2 === 0) for (let i = 0; i < n; i++) {
          const echec = mode === 'echec' && i === 1 && index === 7;
          emettre('offload://evenement', { type: 'copie', destination: i, index, chemin: fichiers[index].chemin, niveau: echec ? 'echec' : 'support', erreur: echec ? { code: 'empreinte-differente', detail: '' } : null, message: echec ? 'La copie ne correspond pas à la carte (empreinte différente)' : undefined, action: echec ? 'Ne pas formater la carte. Recommencer vers un nouveau dossier.' : undefined });
        }
        if (mode === 'lent' && k === 20) emettre('offload://evenement', { type: 'diagnostic', lent: true, goulot: { destination: 1 }, faits: ['La copie a attendu BACKUP pendant 71 % du temps (2 min). C\'est ce disque qui donne le rythme.', 'BACKUP : 36,0 Mo/s en moyenne à l\'écriture.', 'BACKUP est formaté en exfat.'], hypotheses: [{ titre: 'Le disque est peut-être branché sur un port USB 2', parce_que: 'le débit plafonne à 36,0 Mo/s et reste très régulier', verifier: 'Rebrancher sur un autre port, de préférence marqué SS ou bleu, et comparer le débit affiché.' }] });
      }
      emettre('offload://evenement', { type: 'phase', phase: 'rapports' });
      await attendre(200);
      const verdict = annule ? 'annule' : mode === 'echec' ? 'echec' : n >= 2 ? 'bon-a-formater' : 'conforme-ne-pas-formater';
      const remarques = verdict === 'echec' ? ['BACKUP : copie en échec.'] : n < 2 ? ['Une seule copie vérifiée : il en faut deux, sur deux disques différents, avant de formater la carte.'] : [];
      return { issue: 'rapport', noms: demande.bases.map((b) => b.split('/')[2]), remarques, problemes: verdict === 'echec' ? [{ chemin: fichiers[7].chemin, destination: 1, message: 'La copie ne correspond pas à la carte (empreinte différente)', action: 'Ne pas formater la carte. Recommencer vers un nouveau dossier ; si l\'erreur revient sur le même disque, changer de câble puis de disque.' }] : [],
        rapport: { verdict, annule, nb_fichiers: fichiers.length, octets: total, nb_clips: 6, debut: new Date(Date.now() - 360000).toISOString(), fin: new Date().toISOString(),
          destinations: demande.bases.map((b, i) => ({ dossier: `${b}/2026-10-22_A001`, fichiers_ok: fichiers.length - (verdict === 'echec' && i === 1 ? 1 : 0), fichiers_echec: verdict === 'echec' && i === 1 ? 1 : 0, profil: { debit_moyen: debits[i % 3] }, erreur_fatale: null })) },
        ejection: null };
    },
    annuler: async () => { annule = true; return true; },
    verifierDossier: async (chemin) => {
      for (let i = 0; i < 18; i++) { await attendre(40); emettre('offload://verification', { index: i, nb_fichiers: 18, chemin: fichiers[i].chemin, octets: i * 4.1e9, total }); }
      return { dossier: chemin, format: 'ascmhl', manifestes: ['ascmhl/0001_A001_2026-10-21_091512Z.mhl'], nb_fichiers: 18, octets: total, conformes: 18, problemes: [], inattendus: [], conforme: true, problemes_texte: [], annule: false };
    },
    registre: async () => [{ date: '2026-10-21T19:02:00Z', carte: 'A001', nb_fichiers: 18, octets: total, destinations: ['/Volumes/SSD_PROD/Rushs/2026-10-21_A001', '/Volumes/BACKUP/Rushs/2026-10-21_A001'], verdict: 'bon-a-formater' }, { date: '2026-10-21T14:40:00Z', carte: 'B002', nb_fichiers: 212, octets: 512e9, destinations: ['/Volumes/SSD_PROD/Rushs/2026-10-21_B002'], verdict: 'conforme-ne-pas-formater' }],
    ouvrir: async () => {},
    ejecter: async () => {},
    infos: async () => ({ version: '0.2.0', os: 'macos' }),
    ecouter: async (nom, f) => { (ecouteurs[nom] = ecouteurs[nom] || []).push(f); return () => {}; },
    surDepot: async () => () => {},
    notifier: async () => {},
  };
}
