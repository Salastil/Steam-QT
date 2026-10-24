# Steam-QT design: what we build vs. what we borrow

Scope: log in; install/update/verify/uninstall/launch games (native and Proton); automatic
updates; DLC content; workshop downloads; a `steamclient.so` games can load. No store, community,
friends UI or overlay. Purchases open in the default browser.

The decompiled client (`git.salastil.com/Salastil/steam`) is the **reference for correctness**:
we take proto definitions, EMsg values, file formats and interface vtables from it. The **code**
should follow existing open-source implementations, which have years of real-world fixes.

| Area | Primary reference | Lang / licence | How we use it |
|---|---|---|---|
| CM connection, login, unified services | [steam-vent](https://github.com/icewind1991/steam-vent) | Rust, MIT | Depend on it directly if its API fits; otherwise vendor/fork its connection + auth modules. Cross-check against SteamKit2. |
| Protocol behaviour (everything) | [SteamKit2](https://github.com/SteamRE/SteamKit) | C#, LGPL-2.1 | Reference for message flows: PICS, licences, CDN auth tokens, depot keys, workshop. Port logic, don't copy code. |
| Depot/manifest/chunk download | [DepotDownloader](https://github.com/SteamRE/DepotDownloader), [steamroom](https://docs.rs/steamroom) | C# GPL-2.0 / Rust | DepotDownloader is the canonical algorithm (manifest decrypt, chunk AES+LZMA/zstd, delta via old manifest, CDN server rotation). steamroom is a Rust starting point to evaluate. |
| Fallback download engine | SteamCMD (Valve, proprietary, free) | binary | Optional backend: `steamcmd +force_install_dir <lib> +login <u> +app_update <id> validate`. Fetched at runtime from Valve, never bundled. Useful as a correctness oracle in tests. |
| Library folders on disk | Steam's own `libraryfolders.vdf` / `appmanifest_<id>.acf` format (keyvalues-parser crate) | — | Write the same files Steam writes, so libraries stay interchangeable with the official client and with tools like Protontricks. |
| Proton / runtime launch | Proton's `proton` script + Steam Linux Runtime `_v2-entry-point`; [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) | Python GPL-3.0 | umu shows exactly which env Steam sets (`STEAM_COMPAT_DATA_PATH`, `STEAM_COMPAT_CLIENT_INSTALL_PATH`, `SteamAppId`, `waitforexitandrun`, SLR sniper). Proton and SLR are installed as normal Steam tool apps (e.g. 1493710 Experimental, 1628350 sniper) through our own downloader. |
| `steamclient.so` for games | [gbe_fork](https://github.com/Detanup01/gbe_fork) (Goldberg) + Proton [`lsteamclient`](https://github.com/ValveSoftware/Proton/tree/proton_9.0/lsteamclient) | C++ LGPL-3.0 / BSD-style | gbe_fork has every ISteam* interface version and vtable layout; lsteamclient shows what Proton's Windows-side DLL expects from the Linux `steamclient.so`. Our Rust cdylib forwards ownership, app/encrypted tickets and auth tickets to the logged-in session over local IPC instead of emulating. Vtable layouts verified against the decompiled `linux64_steamclient.so`. |
| UI | Kirigami (KF6) + cxx-qt | QML / Rust | Inherits Plasma theme and light/dark scheme automatically. |

## Library folders (default install location)
- First run: if `~/.local/share/Steam/steamapps/libraryfolders.vdf` exists, offer to import those
  libraries (installed games show up immediately, no re-download). Otherwise propose
  `~/Games/SteamLibrary` and let the user choose.
- Settings → Library folders: add/remove folders, set the **default** (radio), show free space
  and game count per folder.
- Install dialog: folder picker defaulting to the default library, with free space shown.
- Each folder keeps the standard `steamapps/` layout (`common/`, `downloading/`, `compatdata/`,
  `shadercache/`, `workshop/`, `appmanifest_*.acf`).

## Build order (each a separate milestone)
1. Session: steam-vent based login + refresh-token autologin; library folders + import.
2. Library: licences → PICS → model (`oslist`, depots, DLC, branches).
3. Content: DepotDownloader-equivalent download/update/verify; PICS change polling for auto-update.
4. Launch: native via SLR; Windows via Proton + SLR; Proton/SLR installed as tool apps.
5. `steamclient.so` shim (gbe_fork interface tables, lsteamclient compatibility).
6. Workshop: subscribed UGC sync before launch.
