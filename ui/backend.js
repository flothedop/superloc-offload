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
      off.push(await window.__TAURI__.event.listen('offload://debit', (e) => h.onDebit(e.payload)));
      off.push(await window.__TAURI__.event.listen('offload://diagnostic', (e) => h.onDiagnostic(e.payload)));
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

  function diagnosticFactice(opts) {
    const lentNom = opts.destinations[1] ? opts.destinations[1].nom : 'disque 2';
    return {
      lent: true,
      goulot: { Destination: 1 },
      faits: [
        `La copie a attendu ${lentNom} pendant 71 % du temps (4 min). C'est ce disque qui donne le rythme.`,
        `${opts.destinations[0].nom} : 470 Mo/s en moyenne à l'écriture.`,
        `${lentNom} : 37.0 Mo/s en moyenne à l'écriture.`,
        `${lentNom} est formaté en exfat.`,
        `Vérification comprise : chaque octet est écrit puis relu depuis le support (2 min de relecture sur ${lentNom}).`,
      ],
      hypotheses: [
        {
          titre: 'Le disque est peut-être branché sur un port USB 2',
          parce_que:
            'le débit plafonne à 37.0 Mo/s et reste très régulier, ce qui correspond au plafond pratique de l\'USB 2',
          verifier:
            'Rebrancher sur un autre port, de préférence marqué SS ou bleu, et comparer le débit affiché.',
        },
      ],
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

    /** Rejoue un déchargement : débits par disque, échecs, diagnostic. */
    async decharger(opts, h) {
      const { fichiers } = opts.source;
      const nbDest = opts.destinations.length;
      const total = fichiers.reduce((t, f) => t + f.taille, 0);
      const pas = Math.max(1, Math.round(6 / this.vitesse));
      let copies = 0;
      const resultats = [];

      // Débits simulés : le second disque plafonne comme un lien USB 2.
      const lent = opts.scenario === 'lent';
      const debitNominal = (d) => (d === 0 ? 470e6 : lent ? 37e6 : 320e6);
      const faits = new Array(nbDest).fill(0);
      const retard = [];
      const courbes = new Array(nbDest).fill(0).map(() => []);

      const remonterDebits = () => {
        for (let d = 0; d < nbDest; d++) {
          // Un peu de bruit, et davantage sur un disque qui peine.
          const bruit = lent && d === 1 ? 0.12 : 0.05;
          const debit = debitNominal(d) * (1 + (Math.random() - 0.5) * bruit * 2);
          courbes[d].push(debit);
          h.onDebit({ destination: d, debit, cumul: copies, fichiers_faits: faits[d] });
        }
      };

      for (let i = 0; i < fichiers.length; i++) {
        const f = fichiers[i];
        h.onFichier({ index: i, total: fichiers.length, chemin: f.chemin });
        for (let p = 1; p <= pas; p++) {
          await attendre(16 / this.vitesse);
          h.onOctets({
            copies: copies + (f.taille * p) / pas,
            total,
            debit: 480e6 * (1 + (Math.random() - 0.5) * 0.1),
          });
          remonterDebits();
        }
        copies += f.taille;

        for (let d = 0; d < nbDest; d++) {
          const casse = opts.scenario === 'echec' && d === 1 && i >= 9;
          const etat = casse ? 'echec' : 'ok';
          const r = {
            destination: d,
            index: i,
            chemin: f.chemin,
            etat,
            motif: casse
              ? `${opts.destinations[1].nom} : écriture impossible (périphérique absent)`
              : null,
          };
          resultats.push(r);
          // Le disque lent prend visiblement du retard : c'est exactement ce
          // que le découplage permet, chacun avance à son rythme.
          if (lent && d === 1) {
            retard.push(r);
          } else {
            if (!casse) faits[d] += 1;
            h.onResultat(r);
          }
        }
        if (lent && i % 3 === 2) {
          const r = retard.shift();
          if (r) {
            faits[1] += 1;
            h.onResultat(r);
          }
        }

        // Le diagnostic apparaît en cours de copie, pas seulement à la fin.
        if (lent && i === 6) h.onDiagnostic(diagnosticFactice(opts));
      }

      // Le disque lent finit sa file après la fin de la lecture de la carte.
      while (retard.length) {
        const r = retard.shift();
        faits[1] += 1;
        h.onResultat(r);
        h.onDebit({ destination: 1, debit: debitNominal(1), cumul: copies, fichiers_faits: faits[1] });
        await attendre(40 / this.vitesse);
      }

      const echecs = resultats.filter((r) => r.etat === 'echec');
      return {
        conforme: echecs.length === 0,
        nb_fichiers: fichiers.length,
        nb_clips: opts.source.clips,
        octets_total: total,
        debit_source: 480e6,
        destinations: opts.destinations.map((d, i) => ({
          nom: d.nom,
          dossier: `${d.chemin}/${opts.source.nom}`,
          conforme: !echecs.some((e) => e.destination === i),
          fichiers_ok: faits[i],
          fichiers_echec: echecs.filter((e) => e.destination === i).length,
          debit_moyen: debitNominal(i),
          systeme_fichiers: i === 1 ? 'exfat' : 'apfs',
        })),
        avertissements: [],
        diagnostic: lent ? diagnosticFactice(opts) : { lent: false, faits: [], hypotheses: [] },
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
