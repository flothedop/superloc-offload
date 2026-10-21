# Relecture indépendante de la v0.2

Avant livraison, le moteur, le verdict et l'interface ont été relus par un
agent qui n'avait pas écrit le code, avec pour consigne de chercher tout
chemin menant à un faux « bon à formater » ou à une perte de données. Il a
travaillé sur une copie du dépôt, exécuté les tests (avec l'outil de
référence de l'ASC) et écrit ses propres expériences.

## Constats et corrections

| Gravité | Constat | Correction |
|---|---|---|
| Critique | Deux destinations dont le disque physique n'était pas identifié comptaient comme deux disques ; sur macOS, un conteneur APFS (disque synthétique) était pris pour le disque physique. | Ne compte qu'un disque **identifié** ; macOS lit les magasins physiques APFS (`diskutil`) ; Linux remonte LVM/LUKS/RAID ; les volumes anonymes (Btrfs, FUSE) ne sont pas identifiés. Ajouté ensuite : une image disque montée (.dmg, .sparsebundle, VHD, périphérique loop) ou un espace de stockage Windows n'est jamais pris pour un disque physique. Tests ajoutés. |
| Élevée | Une destination sur la carte, hors du dossier source, était acceptée et comptée. | Refus de toute destination sur le volume de la carte ; une copie sur le disque de la carte ne compte jamais. |
| Élevée | Dans la file d'attente, l'échec d'une carte disparaissait derrière la réussite de la suivante. | La file s'arrête sur un échec ; la liste des cartes traitées garde chaque verdict. |
| Moyenne | La sonde d'écriture du contrôle préalable pouvait écrire dans la carte. | Aucune sonde sur une destination refusée. |
| Moyenne | Le dossier `_offload` d'un déchargement précédent était copié puis écrasé ; un échec d'écriture du rapport ne changeait pas le verdict ; un dossier apparu entre le contrôle et la copie pouvait recevoir manifestes et rapports. | `_offload` exclu ; rapports créés sans écrasement ; verdict recalculé si un rapport échoue ; rien n'est écrit dans un dossier que ce déchargement n'a pas créé. |
| Moyenne | La relecture de la carte pouvait relire la mémoire sur macOS. | Éviction et mesure avant relecture ; relecture non prouvée signalée. |
| Moyenne | Niveau « relu depuis le disque » accordé sur des systèmes où un autre cache peut répondre (réseau, FUSE, ZFS). | Liste blanche de systèmes de fichiers ; lecteurs réseau Windows détectés ; sous Linux, `O_DIRECT` **et** mesure. |
| Moyenne | Son de réussite pour « ne pas formater encore » ; bandeau vert pour une simple vérification de dossier. | Trois sons distincts ; vérification de dossier sans vert plein. |
| Moyenne | Une carte changée dans le lecteur pendant la file pouvait être copiée sous le nom de la précédente. | Empreinte de la carte vérifiée au lancement. |
| Faible | Carte sans fichier « bonne à formater » ; fichiers spéciaux ignorés en silence ; lecture tronquée non détectée ; attente infinie possible en fin de copie ; renommage non atomique ; temporaires non signalés ; date d'accès de la carte modifiée sous Linux ; vérification C4 et noms HFS+ ; motif d'exclusion non échappé ; noms CONIN$/CONOUT$ ; éjection après échec. | Tous corrigés. |

## Limites connues, non corrigées

- Un disque qui ne répond plus du tout (montage réseau bloqué, disque
  arraché pendant un appel système) peut bloquer la fin de la copie : le
  système ne rend pas la main. L'annulation n'y peut rien.
- Le comportement réel sur macOS (mesure de résidence) et Windows n'est
  vérifié que par l'intégration continue, et sur un vrai déchargement.
