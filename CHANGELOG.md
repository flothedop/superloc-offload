# Journal des versions

## 0.2.0 — reconstruction

Refonte complète à partir d'un audit de la v0.1 et de quatre volets de
recherche (voir `docs/CONCEPTION.md`).

### Fiabilité
- Relecture fondée sur une **mesure** de la résidence en cache après
  éviction ; la v0.1 se fiait au seul drapeau `F_NOCACHE`, insuffisant sur
  macOS. Niveau de vérification consigné par fichier.
- Synchronisation `F_FULLFSYNC` avec repli consigné (exFAT sur Mac).
- Verdict **BON À FORMATER** strict : deux disques physiques distincts.
- Carte modifiée pendant la lecture détectée ; lecture jusqu'à la fin réelle
  des fichiers (volumes Codex HDE).
- Contrôles préalables : FAT32, noms Windows, collisions Unicode et de casse,
  disque en lecture seule, espace par volume.
- Dates de modification, dossiers vides et fichiers de 0 octet conservés.
- Annulation propre ; mise en veille empêchée pendant la copie.

### Standards
- **ASC MHL v2.0** (générations, chaîne C4, empreintes de dossiers), validé
  par l'outil de référence de l'ASC ; MHL v1.1 en plus.
- xxh128 + xxh64 par défaut ; xxh3, MD5, SHA-1, C4 au choix.

### Application
- Détection des cartes, destinations mémorisées, gabarit de nommage, file
  d'attente, son et notification de fin, éjection.
- Vérification d'un disque (ASC MHL ou MHL v1.1), historique des cartes
  déchargées, réglages, français et anglais.
- Rapport HTML imprimable, rapport JSON et journal sur chaque disque.
- Ligne de commande `offload`.

### Développement
- Moteur, ligne de commande et application séparés ; injection de pannes
  déterministe pendant la copie ; tests sur Linux, macOS et Windows.

## 0.1.0

Première version : copie vérifiée vers plusieurs disques, MHL v1.1, débit par
destination et diagnostic de lenteur.
