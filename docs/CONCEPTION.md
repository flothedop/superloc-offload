# SuperLoc Offload — dossier de conception v0.2

Ce document fixe ce que l'outil promet, comment il tient cette promesse et ce
qu'il ne fait pas. Il a été écrit après un audit de la v0.1 et quatre volets de
recherche (concurrence, standards et cartes, pièges système, distribution).
Toute affirmation technique ci-dessous est sourcée dans `docs/SOURCES.md`.

## 1. Le produit en une phrase

Un outil gratuit, pour Mac et Windows, qui décharge une carte caméra vers
plusieurs disques à la fois, relit chaque copie depuis le disque, écrit les
manifestes standard de la profession (ASC MHL) et dit, sans ambiguïté, si la
carte peut être formatée.

Public : clients de SuperLoc (assistants caméra, DIT occasionnels, petites
productions). Ils ne sont pas tous des DIT : l'outil doit être sûr par défaut,
et ne demander aucun réglage pour bien faire.

## 2. Principes non négociables

1. **Ne jamais affirmer plus que ce qui a été prouvé.** Chaque fichier porte un
   niveau de vérification mesuré (§ 5). Un voyant vert ne peut venir que d'une
   relecture depuis le support.
2. **Ne jamais écrire sur la carte.** RED le demande explicitement, et c'est la
   seule garantie qu'une erreur de l'outil ne détruit pas l'original. La source
   est ouverte en lecture seule ; aucun manifeste n'y est déposé.
3. **Ne jamais écraser.** Une copie existante n'est jamais remplacée ; un
   fichier n'obtient son nom définitif qu'après vérification.
4. **Rien ne sort de la machine.** Aucun appel réseau par défaut : ni
   statistiques, ni vérification de mise à jour sans accord.
5. **Les standards sont prouvés, pas supposés.** Les manifestes ASC MHL écrits
   par l'outil sont relus par l'outil de référence de l'ASC (`ascmhl`) dans
   l'intégration continue.
6. **Chaque échec nomme le fichier, la destination et ce qu'il faut faire.**
7. **Les réglages par défaut sont les plus sûrs.** La vitesse est un réglage,
   la sécurité n'en est pas un.

## 3. Ce que la v0.1 faisait bien (conservé)

- Une seule lecture de la carte, écritures parallèles, chaque disque à son
  rythme (mesuré : −30 à −48 % de temps total).
- Fichier temporaire `.offload-part`, renommé seulement après vérification ;
  création exclusive (`create_new`).
- Diagnostic de lenteur qui impute le goulot par temps d'attente mesuré et
  sépare les faits des hypothèses.
- Interface centrée sur un bandeau d'état ; le vert et le rouge ne décorent
  jamais.
- Repli bruyant si le pont entre l'interface et le moteur manque.

## 4. Ce qui change, et pourquoi

| Constat (audit / recherche) | Décision v0.2 |
|---|---|
| Sur macOS, `F_NOCACHE` n'empêche pas de relire des pages déjà en cache (XNU `cluster_read_direct`) : la v0.1 a pu vérifier la RAM. | Écriture et relecture sans cache, **mesure** de la résidence des pages avant relecture (`mincore`), éviction (`msync(MS_INVALIDATE)`), niveau consigné par fichier. Test CI sur macOS qui mesure, et non qui vérifie un drapeau. |
| `sync_all` sur macOS = `F_FULLFSYNC` sans repli ; exFAT n'est pas listé. | Synchronisation maison : `F_FULLFSYNC`, repli `fsync` + durabilité « dégradée » consignée. |
| Dates de modification perdues. | Conservées sur les fichiers et les dossiers (tolérance 2 s pour FAT/exFAT). |
| Carte à la racine d'un lecteur → nom « carte ». | Nom = étiquette du volume ; collision → suffixe proposé. |
| Dossiers vides perdus. | Recréés ; fichiers de 0 octet copiés et listés. |
| Pas de contrôle FAT32 4 Gio, ni des noms interdits sous Windows, ni des collisions de casse / Unicode. | Contrôles préalables bloquants, avant toute écriture. |
| Fichier source qui change pendant la lecture : non détecté. Codex HDE : taille 0 au `stat`. | Lecture jusqu'à la fin réelle ; taille et date relevées avant et après ; changement → échec nommé. |
| MD5 dans le fil de lecture bride les cartes rapides. | Un fil par algorithme d'empreinte. |
| Une allocation par bloc ; un événement par bloc. | Réserve de tampons alignés recyclés (qui borne aussi la mémoire) ; événements limités à 10 par seconde. |
| Pas d'annulation, pas de veille empêchée. | Annulation propre ; mise en veille empêchée pendant la copie. |
| MHL v1.1 seulement. | **ASC MHL v2.0 par défaut**, MHL v1.1 en plus (activée par défaut : Netflix et OffShoot la lisent). |
| xxh64 / md5 seulement. | xxh128 + xxh64 par défaut en une lecture ; xxh3, md5, sha1, c4 au choix. |
| Rapport et MHL écrits hors du moteur, en double. | Le moteur produit tout : manifestes, rapport JSON, rapport HTML imprimable, journal. |
| Erreurs = chaînes françaises. | Codes typés, messages FR / EN. |
| Pas d'injection de pannes. | Plan de pannes déterministe : bit retourné à l'écriture, écriture courte, disque plein, secteur illisible, source modifiée, disque débranché. |
| Rien n'empêche de formater une carte jamais sauvegardée. | Verdict **BON À FORMATER** strict + registre local des cartes déchargées. |

## 5. Niveaux de vérification

Chaque fichier, sur chaque destination, reçoit un niveau :

| Niveau | Signification | Voyant |
|---|---|---|
| `support` | Relu depuis le disque, cache contourné et absence de pages en cache mesurée (macOS) ou garantie par le système (Linux `O_DIRECT`, Windows `NO_BUFFERING`). Empreinte identique. | vert |
| `cache-possible` | Empreinte identique, mais le système n'a pas permis de prouver que la relecture venait du disque. | orange |
| `taille` | Mode rapide : taille seule comparée. | orange, « ne pas formater » |
| `echec` | Empreinte différente, fichier illisible, écriture refusée… | rouge |

Le cache interne du disque (DRAM du SSD) n'est contournable par aucun logiciel :
seule l'éjection suivie d'une revérification le prouve. L'outil le dit et
propose « Vérifier un dossier » après rebranchement.

### Verdict de la carte

- **BON À FORMATER** : tous les fichiers sont au niveau `support` sur **au moins
  deux destinations situées sur deux disques physiques différents**, en mode de
  vérification complet, sans annulation, et sans avertissement bloquant.
- **COPIE CONFORME — NE PAS FORMATER ENCORE** : tout est vérifié mais une seule
  copie (ou deux copies sur le même disque physique), ou niveau `cache-possible`.
- **ÉCHEC** : au moins un fichier en échec sur au moins une destination, avec la
  liste.
- **ANNULÉ** : jamais bon à formater.

### Modes

- **Complet** (défaut) : écriture, synchronisation, relecture de chaque
  destination.
- **Complet + relecture de la carte** : relit aussi la carte à la fin (détecte
  un lecteur de cartes défaillant qui renverrait des données différentes à
  chaque lecture).
- **Taille seule** : rapide, jamais « bon à formater ».

## 6. Moteur

### 6.1 Flux

```
carte ──lecture──► réserve de tampons (alignés, recyclés, bornée)
                     │
         ┌───────────┼──────────────┬──────────────┐
         ▼           ▼              ▼              ▼
   empreinte xxh128  empreinte xxh64   destination A    destination B
   (un fil par algo)                écrit → synchronise → évince → relit → compare → renomme
```

- La lecture prend un tampon libre dans la réserve ; s'il n'y en a pas, elle
  attend : c'est la contre-pression. Le temps d'attente est imputé à la
  destination qui a le plus de blocs en retard : c'est elle le goulot.
- Les empreintes source sont des « promesses » : chaque destination relit son
  fichier puis attend l'empreinte source pour comparer.
- Algorithme de comparaison : le plus fort retenu parmi xxh128 > xxh3 > xxh64 >
  sha1 > md5 > c4. Les autres empreintes de la source sont consignées dans les
  manifestes.

### 6.2 Écriture d'un fichier sur une destination

1. Création exclusive de `nom.ext.offload-part`.
2. Écriture (macOS : `F_NOCACHE` ; ailleurs : tamponnée).
3. Synchronisation (`F_FULLFSYNC` → repli `fsync` consigné ; Windows
   `FlushFileBuffers`).
4. Date de modification de la source appliquée.
5. Éviction du cache (Linux `fadvise(DONTNEED)`, macOS `msync(MS_INVALIDATE)`),
   puis mesure de la résidence (`mincore`).
6. Relecture sans cache et empreinte.
7. Comparaison à l'empreinte source.
8. Renommage vers le nom définitif (patience face aux verrous de l'antivirus
   sous Windows) ; en cas d'échec, suppression du temporaire.

### 6.3 Contrôles préalables (bloquants sauf mention)

- Destination déjà existante → refus, nom libre proposé.
- Destination dans la source, source dans une destination, deux destinations
  identiques.
- Espace libre (+ marge de 1 % et 64 Mio pour les manifestes).
- Fichier > 4 Gio vers FAT32.
- Noms invalides pour la destination (Windows / FAT / exFAT : `< > : " / \ | ? *`,
  caractères 0–31, `CON PRN AUX NUL COM1–9 LPT1–9`, point ou espace final).
- Collisions de noms après normalisation NFC et repli de casse, vers un volume
  insensible à la casse (exFAT, APFS par défaut, NTFS).
- Avertissements non bloquants : deux destinations sur le même disque physique,
  destination réseau, disque USB 2.

### 6.4 Inventaire de la carte

- Tout est copié, y compris fichiers de 0 octet et dossiers vides.
- Exclus (et consignés dans `<ignore>`) : `.fseventsd`, `.Spotlight-V100`,
  `.Trashes`, `.TemporaryItems`, `.DocumentRevisions-V100`, `.DS_Store`, `._*`,
  `System Volume Information`, `$RECYCLE.BIN`.
- Liens symboliques : non suivis, consignés.
- Regroupement en clips (RED `.RDC`, segments `_001`…, ARRI bobines, etc.) pour
  l'affichage et le rapport ; segment manquant dans une séquence → avertissement.

### 6.5 Ce que l'outil écrit sur chaque destination

```
<dossier du déchargement>/
  ... copie conforme de la carte ...
  ascmhl/
    0001_<dossier>_<AAAA-MM-JJ>_<HHMMSS>Z.mhl
    ascmhl_chain.xml
  <dossier>_<AAAA-MM-JJ>_<HHMMSS>.mhl     (MHL v1.1, option activée par défaut)
  _offload/
    rapport.html                           (imprimable en PDF)
    rapport.json
    journal.log
```

Les fichiers de l'outil sont listés dans `<ignore>` du manifeste ASC MHL, pour
que `ascmhl verify` ne les signale pas comme inconnus.

### 6.6 Annulation, panne, veille

- Annulation : arrêt de la lecture, suppression des temporaires en cours ; les
  fichiers déjà vérifiés restent ; rapport « ANNULÉ ».
- Disque débranché : la destination est marquée en échec avec ses fichiers ;
  les autres continuent.
- Erreur de lecture de la carte : le fichier est en échec partout, le
  déchargement continue sur les fichiers suivants, verdict ÉCHEC.
- Veille empêchée pendant la copie (le capot fermé endort quand même : l'outil
  le dit).

### 6.7 Registre local

Un registre JSON dans le dossier de données de l'application garde, pour chaque
déchargement : empreinte de la carte (étiquette + liste des chemins, tailles,
dates), date, destinations, verdict. À l'insertion d'une carte connue :
« déjà déchargée le … vers … ». Aucune donnée ne quitte la machine.

## 7. Application

- Détection des cartes insérées (scrutation des volumes amovibles), jamais de
  démarrage automatique.
- Destinations favorites mémorisées ; gabarit de nommage à jetons :
  `{carte}`, `{date}`, `{projet}`, `{jour}`, `{camera}`, `{operateur}`.
- File d'attente : plusieurs cartes à la suite.
- Pendant la copie : débit par destination, temps restant, diagnostic en
  direct, bouton Annuler.
- Fin : son distinct (réussite / échec), notification système, verdict, accès
  au rapport, éjection de la carte (option).
- Onglet « Vérifier un dossier » : relit un déchargement à partir de son ASC MHL
  (ou de son MHL v1.1) et liste manquants, modifiés, inattendus.
- Français / anglais.
- Journal affiché par clips et limité en DOM (cartes de 5 000 fichiers).

## 8. Distribution

- macOS : sans signature Apple, l'ouverture passe par Réglages > Confidentialité
  et sécurité > « Ouvrir quand même » (le clic droit > Ouvrir ne marche plus
  depuis macOS 15). **Recommandation : compte Apple Developer au nom de la SARL
  (99 USD/an) ; la CI signe et notarise dès que les secrets sont présents.**
- Windows : SmartScreen « Exécuter quand même ». Option : Azure Artifact
  Signing (9,99 USD/mois). La CI signe si les secrets sont présents.
- Pas de mise à jour automatique en v0.2 (elle impose une clé de signature à ne
  jamais perdre et un appel réseau à déclarer).

## 9. Responsabilité

- Avertissement affiché au premier lancement et dans chaque rapport : outil
  d'aide, garder la carte tant que deux copies vérifiées n'existent pas.
- La directive (UE) 2024/2853 fait du logiciel un produit à partir du
  9 décembre 2026, y compris pour la corruption de données ; un outil diffusé
  par une société dans le cadre de son activité est probablement concerné. À
  valider avec un juriste avant diffusion large.
- Licence : décision de SuperLoc (MIT actuelle ; Apache-2.0 + `TRADEMARKS.md`
  conseillé pour protéger le nom).

## 10. Vérification du projet

- Tests unitaires et d'intégration du moteur sur Linux, macOS et Windows.
- Injection de pannes à chaque étape (§ 4).
- Test de résidence du cache sur macOS (mesure `mincore` après notre
  écriture).
- Interopérabilité : `ascmhl verify` et validation XSD sur nos manifestes ;
  vecteurs de test des empreintes.
- Relecture indépendante du code par un agent qui n'a pas écrit le moteur.
- **Reste à faire hors CI : un vrai déchargement de carte RED, puis éjection,
  rebranchement et « Vérifier un dossier ».**

## 11. Hors v0.2 (feuille de route)

Reprise d'un déchargement interrompu ; copie en cascade ; vignettes et
métadonnées caméra ; PDF natif ; mise à jour automatique ; Codex HDE (exige
CODEX Platform) ; notification sur téléphone ; préréglages livrés avec la
location (QR du devis) ; remontée volontaire de la santé des cartes du parc.
