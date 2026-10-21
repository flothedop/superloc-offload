# Sources

Sources consultées en octobre 2026 pour la conception de la v0.2. Les
affirmations techniques de `CONCEPTION.md` s'appuient sur elles ; les points
non vérifiés y sont signalés comme tels.

## ASC MHL et empreintes

- Spécification ASC MHL v1.0 (format v2.0), 15 mars 2022 — https://cms-assets.theasc.com/ASCMHL_Specification_v1.0.pdf
- Schémas XSD et implémentation de référence `ascmhl` — https://github.com/ascmitc/mhl
- Documentation de l'outil de référence (commandes `create`, `diff`, `verify`, `xsd-schema-check`) — https://ascmhl.readthedocs.io/en/latest/README.html
- Netflix, gestion des données de production (MHL xxHash64) — https://backlothelp.netflix.com/hc/en-us/articles/360000581207-Production-Assets-Data-Management
- Netflix Footage Upload, gestion des médias — https://backlothelp.netflix.com/hc/en-us/articles/16533000584979-Netflix-Footage-Upload-Media-Management
- Frame.io, flux de travail MHL — https://blog.frame.io/2022/08/22/mhl-media-hash-lists-workflow/

Les vecteurs de test (chaîne vide, « abc ») et les empreintes de dossiers sont
recoupés avec `ascmhl` 1.2 exécuté localement ; les tests le vérifient.

## Système : cache, écriture physique, volumes

- XNU, `cluster_read_direct` (pages en cache servies malgré `F_NOCACHE`) — https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_cluster.c
- XNU, `fcntl(2)` (`F_NOCACHE`, `F_FULLFSYNC`, systèmes de fichiers concernés) — https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/fcntl.2
- XNU, `msync(2)` (`MS_INVALIDATE`) — https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/msync.2
- Eclectic Light, « How can you trust a disk to write data? » — https://eclecticlight.co/2022/02/18/how-can-you-trust-a-disk-to-write-data/
- Eclectic Light, FSKit et systèmes de fichiers de macOS 26 — https://eclecticlight.co/2025/11/18/which-local-file-systems-does-macos-26-support/
- Microsoft, mise en mémoire tampon des fichiers (`FILE_FLAG_NO_BUFFERING`) — https://learn.microsoft.com/en-us/windows/win32/FileIO/file-buffering
- Microsoft, mise en cache des fichiers — https://learn.microsoft.com/en-us/windows/win32/FileIO/file-caching
- Raymond Chen, `FlushFileBuffers` et le cache du disque — https://devblogs.microsoft.com/oldnewthing/20170510-00/?p=95505
- Microsoft, `CM_Request_Device_EjectW` — https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_request_device_ejectw
- Apple, `NSRemovableVolumesUsageDescription` — https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSRemovableVolumesUsageDescription
- Apple, DiskArbitration — https://developer.apple.com/documentation/diskarbitration
- Apple, assertions de mise en veille (IOPM) — https://developer.apple.com/documentation/iokit/iopmlib_h/iopmassertiontypes
- Crate `keepawake` — https://crates.io/crates/keepawake

## Distribution

- macOS Sequoia : fin du contournement par clic droit > Ouvrir (9to5Mac, 6 août 2024) — https://9to5mac.com/2024/08/06/macos-sequoia-makes-it-harder-to-run-apps-that-dont-follow-apples-security-rules/
- Apple, notarisation — https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- Apple, Developer ID — https://developer.apple.com/developer-id/
- Apple, D-U-N-S — https://developer.apple.com/help/account/membership/D-U-N-S/
- Microsoft Azure Artifact Signing — https://azure.microsoft.com/en-us/products/artifact-signing

## Droit

- Directive (UE) 2024/2853 relative à la responsabilité du fait des produits défectueux — https://eur-lex.europa.eu/eli/dir/2024/2853/oj/fra

## Concurrence et usages

- OffShoot, modes de vérification — https://docs.hedge.video/offshoot/features/verification
- OffShoot, rapports — https://docs.hedge.video/offshoot/features/reports
- DaVinci Resolve, Clone Tool — https://dvresolve.com/tutorial/copying-footage-clone-tool/
