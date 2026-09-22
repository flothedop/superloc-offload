/* ==========================================================================
   Pont vers le moteur.
   Dans l'application, les appels partent vers le code Rust via Tauri.
   Hors application (navigateur), un simulateur prend le relais : il sert à
   travailler l'interface et à la relire sans carte ni disque.
   ========================================================================== */

const Backend = (() => {
  const passerelle = typeof window !== 'undefined' && window.__TAURI__;
  // Présent dès qu'on s'exécute dans l'application, même si la passerelle
  // publique n'est pas exposée.
  const dansApplication = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

  if (dansApplication && !passerelle) {
    // Cas critique : on est bien dans l'application, mais le pont vers le
    // moteur manque. Simuler ici afficherait un « Copie conforme » alors
    // qu'aucun fichier n'aurait été copié. On refuse de démarrer.
    const erreur = "Le pont vers le moteur de copie est absent : l'application ne peut pas fonctionner.";
    document.addEventListener('DOMContentLoaded', () => {
      document.body.innerHTML =
        '<div style="padding:40px;font-family:sans-serif;color:#ff9a92;background:#14181a;height:100%">'
        + '<h1 style="font-size:24px">Application inutilisable</h1><p>' + erreur + '</p>'
        + '<p>Ne pas s\'en servir pour décharger une carte.</p></div>';
    });
    const refus = () => Promise.reject(new Error(erreur));
    return {
      simule: false, enPanne: true,
      choisirCarte: refus, choisirDisque: refus, decharger: refus,
      verifierDossier: refus, ouvrirDossier: refus,
    };
  }

  // ----------------------------------------------------------------- Tauri
  const reel = {
    simule: false,
    choisirCarte: (chemin) => window.__TAURI__.core.invoke('choisir_carte', { chemin: chemin || null }),
    choisirDisque: (chemin) => window.__TAURI__.core.invoke('choisir_disque', { chemin: chemin || null }),
    ouvrirDossier: (c) => window.__TAURI__.core.invoke('ouvrir_dossier', { chemin: c }),
    async decharger(opts, h) {
      const off = [];
      off.push(await window.__TAURI__.event.listen('offload://fichier', (e) => h.onFichier(e.payload)));
      off.push(await window.__TAURI__.event.listen('offload://octets', (e) => h.onOctets(e.payload)));
      off.push(await window.__TAURI__.event.listen('offload://verification', (e) => h.onVerif(e.payload)));
      off.push(await window.__TAURI__.event.listen('offload://resultat', (e) => h.onResultat(e.payload)));
      try {
        return await window.__TAURI__.core.invoke('decharger', { options: opts });
      } finally {
        off.forEach((f) => f());
      }
    },
    verifierDossier: (chemin) => window.__TAURI__.core.invoke('verifier_dossier', { chemin: chemin || null }),
  };

  // ------------------------------------------------------------ Simulateur
  const Go = 1000 * 1000 * 1000;
  const attendre = (ms) => new Promise((r) => setTimeout(r, ms));

  function carteFactice() {
    const fichiers = [];
    // Douze plans RED : chaque plan est un dossier .RDC découpé en segments.
    for (let c = 1; c <= 12; c++) {
      const clip = `A001_C${String(c).padStart(3, '0')}_0922JK.RDC`;
      const segments = c % 4 === 0 ? 5 : c % 3 === 0 ? 4 : 3;
      for (let s = 1; s <= segments; s++) {
        fichiers.push({
          chemin: `${clip}/A001_C${String(c).padStart(3, '0')}_0922JK_${String(s).padStart(3, '0')}.R3D`,
          clip,
          taille: 8 * Go + (s * 137 * 1000 * 1000) % (2 * Go),
        });
      }
      fichiers.push({ chemin: `${clip}/A001_C${String(c).padStart(3, '0')}_0922JK.RMD`, clip, taille: 24 * 1024 });
    }
    const octets = fichiers.reduce((t, f) => t + f.taille, 0);
    return {
      nom: 'A001_0922JK',
      chemin: '/Volumes/A001_0922JK',
      structure: 'RED · KOMODO',
      clips: 12,
      fichiers,
      octets,
    };
  }

  let compteurDisques = 0;
  const disquesFactices = [
    { nom: 'RAID_TOURNAGE', chemin: '/Volumes/RAID_TOURNAGE', total: 8000 * Go, libre: 5120 * Go },
    { nom: 'BACKUP_PROD', chemin: '/Volumes/BACKUP_PROD', total: 4000 * Go, libre: 1340 * Go },
    { nom: 'SSD_MONTAGE', chemin: '/Volumes/SSD_MONTAGE', total: 2000 * Go, libre: 210 * Go },
  ];

  const simule = {
    simule: true,
    vitesse: 1,
    async choisirCarte() {
      await attendre(120);
      return carteFactice();
    },
    async choisirDisque() {
      await attendre(120);
      return disquesFactices[compteurDisques++ % disquesFactices.length];
    },
    estReel: false,
    async ouvrirDossier() {},

    /** Rejoue un déchargement, y compris un scénario d'échec. */
    async decharger(opts, h) {
      const { fichiers } = opts.source;
      const nbDest = opts.destinations.length;
      const total = fichiers.reduce((t, f) => t + f.taille, 0);
      const pas = Math.max(1, Math.round(6 / this.vitesse));
      let copies = 0;
      const resultats = [];

      for (let i = 0; i < fichiers.length; i++) {
        const f = fichiers[i];
        h.onFichier({ index: i, total: fichiers.length, chemin: f.chemin });
        for (let p = 1; p <= pas; p++) {
          await attendre(16 / this.vitesse);
          h.onOctets({ copies: copies + (f.taille * p) / pas, total });
        }
        copies += f.taille;
        h.onVerif({ chemin: f.chemin, index: i });
        await attendre(30 / this.vitesse);

        // Scénario d'échec : le second disque lâche au dixième fichier.
        const etats = [];
        for (let d = 0; d < nbDest; d++) {
          const casse = opts.scenario === 'echec' && d === 1 && i >= 9;
          etats.push(casse ? 'echec' : 'ok');
        }
        const motif = etats.includes('echec')
          ? `${opts.destinations[1].nom} : écriture impossible (périphérique absent)`
          : null;
        resultats.push({ index: i, chemin: f.chemin, etats, motif });
        h.onResultat(resultats[resultats.length - 1]);
      }

      const echecs = resultats.filter((r) => r.etats.includes('echec'));
      return {
        conforme: echecs.length === 0,
        nb_fichiers: fichiers.length,
        nb_clips: opts.source.clips,
        octets_total: total,
        destinations: opts.destinations.map((d, i) => ({
          nom: d.nom,
          dossier: `${d.chemin}/${opts.source.nom}`,
          conforme: !echecs.some((e) => e.etats[i] === 'echec'),
          fichiers_ok: fichiers.length - echecs.filter((e) => e.etats[i] === 'echec').length,
          fichiers_echec: echecs.filter((e) => e.etats[i] === 'echec').length,
        })),
        avertissements: [],
      };
    },

    async verifierDossier() {
      await attendre(500);
      const carte = carteFactice();
      return {
        dossier: '/Volumes/RAID_TOURNAGE/A001_0922JK',
        mhls: ['A001_0922JK_2026-09-22_183012.mhl'],
        lignes: carte.fichiers.map((f, i) => ({
          fichier: f.chemin,
          etat: i === 14 ? 'Echec' : 'Ok',
          detail: i === 14
            ? 'empreinte xxhash64 différente : 3f2ab9c710de4455 à la destination contre ae2ac11508e8286b au MHL'
            : null,
        })),
        absents: [],
        en_trop: [],
        residus: [],
        etat: 'Echec',
      };
    },
  };

  return passerelle ? reel : simule;
})();
