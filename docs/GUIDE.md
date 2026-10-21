# Guide d'utilisation

## Installer

### Mac

1. Ouvrir le fichier `SuperLoc Offload_x.y.z_universal.dmg` et glisser
   l'application dans **Applications**.
2. Si l'application n'est pas signée par Apple (versions de test), macOS
   refuse de l'ouvrir au premier essai. Depuis macOS 15, le clic droit >
   Ouvrir ne suffit plus :
   - lancer l'application une fois (le message de refus s'affiche, le fermer) ;
   - ouvrir **Réglages Système > Confidentialité et sécurité** ;
   - tout en bas, cliquer **Ouvrir quand même** à côté de « SuperLoc
     Offload », puis confirmer avec le mot de passe.
3. Au premier accès à une carte, macOS demande l'autorisation d'accéder aux
   volumes amovibles : accepter.

### Windows

1. Lancer `SuperLoc Offload_x.y.z_x64-setup.exe`.
2. Si Windows affiche « Windows a protégé votre ordinateur », cliquer
   **Informations complémentaires** puis **Exécuter quand même**.

## Premier déchargement

1. **Insérer la carte.** Elle apparaît en haut à gauche, avec son format
   (RED, Sony, ARRI…). Cliquer dessus, ou déposer le dossier de la carte sur
   la fenêtre.
2. **Choisir les disques.** « Ajouter un disque » : choisir le dossier où
   ranger les rushs sur chaque disque (par exemple `Rushs`). Les disques sont
   mémorisés ; décocher ceux qu'on n'utilise pas pour cette carte. La jauge
   montre si la carte tient.
3. **Nommage** (facultatif) : le dossier créé s'appelle par défaut
   `date_carte` (`2026-10-22_A001`). Le gabarit accepte `{projet}`, `{jour}`,
   `{camera}`, `{operateur}` ; un `/` crée un sous-dossier, par exemple
   `{projet}/J{jour}/{carte}`.
4. **Décharger.** Le bandeau se remplit ; chaque disque affiche son débit.
   Pendant la copie, on peut préparer la carte suivante : elle sera copiée
   juste après (« Mettre en file »).
5. **Lire le verdict.** Seul **BON À FORMATER** autorise à formater la carte.

### Conseils

- Deux disques **physiques** différents : deux dossiers sur le même disque
  ne valent qu'une copie, l'outil le signale.
- Ne pas fermer le capot pendant une copie : l'ordinateur s'endormirait
  (l'outil empêche la mise en veille automatique, pas celle du capot).
- Après la copie, **éjecter proprement** les disques avant de les débrancher.
- Pour une sécurité maximale : éjecter un disque, le rebrancher, puis
  l'onglet **Vérifier un disque**. C'est la seule façon de prouver que les
  données ont quitté le cache interne du disque.
- Vérification « Complète + relecture de la carte » : relit aussi la carte à
  la fin. Plus long ; utile si un lecteur de cartes est suspect.

## Vérifier un disque

Onglet **Vérifier un disque** > choisir le dossier d'un déchargement (celui
qui contient `ascmhl/`). L'outil relit chaque fichier et le compare au
manifeste : fichiers manquants, modifiés ou inattendus sont listés. Les
déchargements faits par d'autres outils (Silverstack, ShotPut, OffShoot)
devraient être lisibles dès lors qu'ils ont écrit un ASC MHL ou un MHL v1.1 à
la racine du dossier ; ce cas n'a pas encore été testé sur de vrais fichiers.

## En cas de problème

Chaque problème dit **quel fichier**, **sur quel disque**, et **quoi faire**.
Les cas fréquents :

| Message | Que faire |
|---|---|
| La copie ne correspond pas à la carte | Ne pas formater. Recommencer vers un nouveau dossier ; si l'erreur revient sur le même disque, changer de câble, puis de disque. |
| Lecture de la carte impossible | Ne pas formater. Essayer un autre lecteur de cartes et un autre câble. |
| Le fichier a changé sur la carte pendant la copie | La carte est encore utilisée (caméra, autre logiciel) : la démonter, la remonter, recommencer. |
| Disque plein / Espace libre insuffisant | Libérer de la place ou choisir un autre disque. |
| Fichier de plus de 4 Gio vers un disque FAT32 | Utiliser un disque exFAT, APFS ou NTFS. |
| Disque en lecture seule | Sur Mac, un disque NTFS est en lecture seule : utiliser un disque exFAT ou APFS. |

Le rapport complet (`_offload/rapport.html` sur chaque disque) s'ouvre avec
le bouton **Ouvrir le rapport** et s'imprime en PDF depuis le navigateur. Le
journal technique (`_offload/journal.log`) est à joindre à toute demande
d'aide.

## Limites connues

- **ARRIRAW HDE sur Codex** : nécessite CODEX Platform (Mac) ; l'outil copie
  ce que le système lui présente.
- Les disques réseau sont acceptés, mais la relecture peut y être servie par
  le serveur : l'outil le signale.
- Mises à jour : à télécharger manuellement (aucune connexion n'est établie
  par l'outil).
