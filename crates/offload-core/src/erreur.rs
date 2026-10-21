//! Erreurs typées.
//!
//! Chaque erreur porte un code stable (pour le rapport, les tests et la
//! traduction) et un détail factuel : chemin concerné, message du système.
//! Le message affiché dit ce qui s'est passé **et** ce qu'il faut faire.

use crate::langue::Langue;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Code {
    // Source
    SourceIntrouvable,
    SourceVide,
    LectureSource,
    SourceModifiee,
    SourceInstable,
    NomIllisible,
    // Destination
    CreationImpossible,
    EcritureImpossible,
    DisquePlein,
    SynchroImpossible,
    RelectureImpossible,
    EmpreinteDifferente,
    TailleDifferente,
    RenommageImpossible,
    DestinationPerdue,
    // Déroulement
    Annule,
    Interne,
    // Contrôles préalables
    DestinationExiste,
    DestinationDansSource,
    SourceDansDestination,
    DestinationsIdentiques,
    EspaceInsuffisant,
    FichierTropGrosFat32,
    NomInvalide,
    CollisionNoms,
    AucuneDestination,
    // Vérification d'un dossier
    Manquant,
    Inattendu,
    ManifesteAbsent,
    ManifesteIllisible,
    ManifesteModifie,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Erreur {
    pub code: Code,
    /// Détail factuel : chemin, message du système. Jamais traduit.
    pub detail: String,
}

pub type Resultat<T> = Result<T, Erreur>;

impl Erreur {
    pub fn new(code: Code, detail: impl Into<String>) -> Erreur {
        Erreur {
            code,
            detail: detail.into(),
        }
    }

    /// Construit une erreur à partir d'une erreur système, en reconnaissant
    /// le disque plein.
    pub fn io(code: Code, contexte: impl fmt::Display, e: &io::Error) -> Erreur {
        let code = if est_disque_plein(e) {
            Code::DisquePlein
        } else {
            code
        };
        Erreur::new(code, format!("{contexte} : {e}"))
    }

    /// Message complet : ce qui s'est passé, puis le détail.
    pub fn message(&self, l: Langue) -> String {
        format!("{} ({})", titre(self.code, l), self.detail)
    }

    /// Ce que l'utilisateur doit faire.
    pub fn action(&self, l: Langue) -> &'static str {
        action(self.code, l)
    }
}

impl fmt::Display for Erreur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Langue::Fr))
    }
}

impl std::error::Error for Erreur {}

pub fn est_disque_plein(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::StorageFull {
        return true;
    }
    match e.raw_os_error() {
        // ENOSPC (Linux 28, macOS 28) ; EDQUOT (Linux 122, macOS 69).
        #[cfg(unix)]
        Some(c) => c == libc::ENOSPC || c == libc::EDQUOT,
        // ERROR_HANDLE_DISK_FULL (39), ERROR_DISK_FULL (112).
        #[cfg(windows)]
        Some(c) => c == 39 || c == 112,
        #[cfg(not(any(unix, windows)))]
        Some(_) => false,
        None => false,
    }
}

pub fn titre(code: Code, l: Langue) -> &'static str {
    use Code::*;
    match code {
        SourceIntrouvable => l.t("Source introuvable", "Source not found"),
        SourceVide => l.t(
            "La source ne contient aucun fichier",
            "The source contains no files",
        ),
        LectureSource => l.t("Lecture de la carte impossible", "Card read error"),
        SourceModifiee => l.t(
            "Le fichier a changé sur la carte pendant la copie",
            "The file changed on the card during the copy",
        ),
        SourceInstable => l.t(
            "La carte ne renvoie pas les mêmes données à la relecture",
            "The card returned different data when read again",
        ),
        NomIllisible => l.t("Nom de fichier illisible", "Unreadable file name"),
        CreationImpossible => l.t("Création du fichier impossible", "Cannot create the file"),
        EcritureImpossible => l.t("Écriture impossible", "Write error"),
        DisquePlein => l.t("Disque plein", "Disk full"),
        SynchroImpossible => l.t(
            "Écriture physique sur le disque impossible",
            "Could not flush data to the disk",
        ),
        RelectureImpossible => l.t(
            "Relecture de la copie impossible",
            "Cannot read back the copy",
        ),
        EmpreinteDifferente => l.t(
            "La copie ne correspond pas à la carte (empreinte différente)",
            "The copy does not match the card (checksum mismatch)",
        ),
        TailleDifferente => l.t(
            "La copie n'a pas la taille du fichier de la carte",
            "The copy does not have the size of the card file",
        ),
        RenommageImpossible => l.t(
            "Impossible de donner son nom définitif au fichier vérifié",
            "Cannot give the verified file its final name",
        ),
        DestinationPerdue => l.t("Destination perdue", "Destination lost"),
        Annule => l.t("Copie annulée", "Copy cancelled"),
        Interne => l.t("Erreur interne", "Internal error"),
        DestinationExiste => l.t(
            "Le dossier de destination existe déjà",
            "The destination folder already exists",
        ),
        DestinationDansSource => l.t(
            "La destination se trouve sur la carte",
            "The destination is inside the source",
        ),
        SourceDansDestination => l.t(
            "La carte se trouve dans la destination",
            "The source is inside the destination",
        ),
        DestinationsIdentiques => l.t(
            "La même destination est choisie deux fois",
            "The same destination is selected twice",
        ),
        EspaceInsuffisant => l.t("Espace libre insuffisant", "Not enough free space"),
        FichierTropGrosFat32 => l.t(
            "Fichier de plus de 4 Gio vers un disque FAT32",
            "File larger than 4 GiB for a FAT32 disk",
        ),
        NomInvalide => l.t(
            "Nom de fichier refusé par le disque de destination",
            "File name not allowed on the destination disk",
        ),
        CollisionNoms => l.t(
            "Deux fichiers auraient le même nom sur la destination",
            "Two files would get the same name on the destination",
        ),
        AucuneDestination => l.t("Aucune destination choisie", "No destination selected"),
        Manquant => l.t("Fichier manquant", "Missing file"),
        Inattendu => l.t(
            "Fichier absent du manifeste",
            "File not listed in the manifest",
        ),
        ManifesteAbsent => l.t(
            "Aucun manifeste MHL dans ce dossier",
            "No MHL manifest in this folder",
        ),
        ManifesteIllisible => l.t("Manifeste MHL illisible", "Unreadable MHL manifest"),
        ManifesteModifie => l.t(
            "Le manifeste a été modifié depuis sa création",
            "The manifest was modified after it was written",
        ),
    }
}

pub fn action(code: Code, l: Langue) -> &'static str {
    use Code::*;
    match code {
        SourceIntrouvable => l.t(
            "Vérifier que la carte est bien montée, puis la choisir à nouveau.",
            "Check that the card is mounted, then select it again.",
        ),
        SourceVide => l.t(
            "Vérifier qu'il s'agit de la bonne carte.",
            "Check that this is the right card.",
        ),
        LectureSource | SourceInstable => l.t(
            "Ne pas formater la carte. Essayer un autre lecteur de cartes et un autre câble, puis recommencer.",
            "Do not format the card. Try another card reader and cable, then start again.",
        ),
        SourceModifiee => l.t(
            "Ne pas formater la carte. La carte est peut-être encore utilisée par la caméra ou un autre logiciel : la démonter, la remonter, recommencer.",
            "Do not format the card. It may still be in use by the camera or another program: unmount, remount and start again.",
        ),
        NomIllisible => l.t(
            "Copier ce fichier à la main et le signaler à SuperLoc.",
            "Copy this file manually and report it to SuperLoc.",
        ),
        CreationImpossible | EcritureImpossible | SynchroImpossible | RelectureImpossible
        | DestinationPerdue => l.t(
            "Vérifier le branchement du disque (câble, port, alimentation) puis recommencer vers un nouveau dossier.",
            "Check the disk connection (cable, port, power) then start again into a new folder.",
        ),
        DisquePlein | EspaceInsuffisant => l.t(
            "Libérer de la place ou choisir un autre disque.",
            "Free some space or choose another disk.",
        ),
        EmpreinteDifferente | TailleDifferente => l.t(
            "Ne pas formater la carte. Recommencer vers un nouveau dossier ; si l'erreur revient sur le même disque, changer de câble puis de disque.",
            "Do not format the card. Start again into a new folder; if the error comes back on the same disk, change the cable, then the disk.",
        ),
        RenommageImpossible => l.t(
            "Un autre programme (antivirus, synchronisation) bloque peut-être le fichier. Le fermer et recommencer vers un nouveau dossier.",
            "Another program (antivirus, sync) may be locking the file. Close it and start again into a new folder.",
        ),
        Annule => l.t(
            "Relancer le déchargement vers un nouveau dossier.",
            "Start the offload again into a new folder.",
        ),
        Interne => l.t(
            "Ne pas formater la carte. Envoyer le journal à SuperLoc.",
            "Do not format the card. Send the log to SuperLoc.",
        ),
        DestinationExiste => l.t(
            "Choisir un autre nom : l'outil n'écrase jamais une copie existante.",
            "Choose another name: the tool never overwrites an existing copy.",
        ),
        DestinationDansSource | SourceDansDestination | DestinationsIdentiques
        | AucuneDestination => l.t("Choisir une autre destination.", "Choose another destination."),
        FichierTropGrosFat32 => l.t(
            "Utiliser un disque formaté en exFAT, APFS ou NTFS.",
            "Use a disk formatted as exFAT, APFS or NTFS.",
        ),
        NomInvalide | CollisionNoms => l.t(
            "Choisir un disque formaté en APFS ou en Mac OS étendu, ou renommer le fichier sur une copie.",
            "Use a disk formatted as APFS or Mac OS Extended, or rename the file on a copy.",
        ),
        Manquant | Inattendu => l.t(
            "Comparer avec une autre copie et refaire celle-ci si besoin.",
            "Compare with another copy and redo this one if needed.",
        ),
        ManifesteAbsent | ManifesteIllisible | ManifesteModifie => l.t(
            "Vérifier ce dossier à partir d'une autre copie.",
            "Verify this folder from another copy.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_disque_plein_est_reconnu() {
        let e = io::Error::from(io::ErrorKind::StorageFull);
        assert_eq!(
            Erreur::io(Code::EcritureImpossible, "x", &e).code,
            Code::DisquePlein
        );
    }

    #[test]
    fn chaque_code_a_un_message_et_une_action_dans_les_deux_langues() {
        // Le code sérialisé sert de clé stable dans les rapports.
        let e = Erreur::new(Code::EmpreinteDifferente, "A001.R3D");
        assert!(e.message(Langue::Fr).contains("A001.R3D"));
        assert!(!e.action(Langue::En).is_empty());
        assert_eq!(
            serde_json::to_string(&Code::EmpreinteDifferente).unwrap(),
            "\"empreinte-differente\""
        );
    }
}
