# Pylentra — Design v1

- **Data:** 2026-10-21
- **Stato:** in revisione
- **Identifier:** `me.luucka.pylentra`

## 1. Obiettivo

Pylentra è un gestore di connessioni SSH con terminale integrato, distribuito
come prodotto pubblico per Linux e Windows. L'utente salva le connessioni
(organizzate in gruppi), le apre in schede di terminale, si autentica con
password, chiave o agent, passa per jump host e usa il port forwarding, senza
dipendere dall'OpenSSH installato.

**Criteri di successo della v1**

- Installabile su Linux (AppImage, `.deb`) e Windows (installer NSIS) dalle
  GitHub Release, con aggiornamento automatico.
- Il terminale gestisce correttamente programmi a schermo intero (vim, htop),
  colori, resize, copia e incolla.
- Nessun segreto scritto in chiaro su disco, nei log o negli errori.
- Le host key vengono sempre verificate; una chiave cambiata blocca la
  connessione.

## 2. Decisioni tecniche

| Area | Scelta |
|---|---|
| Linguaggio backend | Rust (tokio) |
| Shell desktop | Tauri 2 |
| Frontend | Svelte 5 + TypeScript + Vite, gestito con pnpm |
| Terminale | xterm.js (con FitAddon) |
| SSH | `russh`, client integrato (niente OpenSSH di sistema) |
| Segreti | keychain di sistema tramite il crate `keyring`, dietro il trait `SecretStore` |
| Persistenza | file TOML |
| Trasporto I/O del terminale | `tauri::ipc::Channel` per sessione (approccio A) |
| Tipi Rust e TS | generati con `tauri-specta` (alternativa: `ts-rs`) |
| Lingue della UI | inglese e italiano |
| Licenza | doppia MIT / Apache-2.0 |

## 3. Scope

**Nella v1**

- CRUD delle connessioni, gruppi piatti, ricerca per nome, host e utente.
- Più sessioni in schede.
- Autenticazione con password (incluso keyboard-interactive), chiave privata
  con passphrase, ssh-agent e Pageant.
- Verifica delle host key con conferma al primo contatto e blocco in caso di
  mismatch.
- Jump host concatenabili (ProxyJump).
- Port forwarding locale e remoto, con avvio automatico o manuale.
- Impostazioni: tema, font del terminale, lingua, cartella dei log.
- Aggiornamento automatico.

**Fuori dalla v1**

- Browser SFTP, previsto per la v2.
- Import di `~/.ssh/config`.
- Forwarding dinamico SOCKS (`-D`).
- Cartelle annidate.
- Export e import delle connessioni.
- Vault cifrato con master password.
- macOS.
- Firma del codice su Windows.
- Test E2E automatici.
- Accorpamento dell'output del terminale in blocchi più grandi (solo se le
  misure lo richiederanno).

## 4. Architettura

```
pylentra/
├── Cargo.toml                  # workspace: crates/pylentra-core, src-tauri
├── crates/pylentra-core/       # logica, nessuna dipendenza da Tauri
│   └── src/
│       ├── model.rs            # Connection, Group, AuthMethod, ForwardRule
│       ├── store.rs            # ConnectionStore (TOML)
│       ├── secrets.rs          # trait SecretStore + KeyringStore
│       ├── known_hosts.rs      # verifica fingerprint
│       ├── ssh/
│       │   ├── session.rs      # SessionManager, PTY, loop di I/O
│       │   ├── auth.rs         # password / chiave / agent
│       │   ├── jump.rs         # jump host via direct-tcpip
│       │   └── forward.rs      # ForwardManager: tunnel locali e remoti
│       └── error.rs            # CoreError
├── src-tauri/                  # comandi Tauri e stato dell'app
│   └── src/{main.rs, commands.rs, state.rs}
└── ui/                         # Svelte 5 + TS
    └── src/
        ├── lib/api.ts          # unico punto di accesso a invoke/Channel
        ├── lib/bindings.ts     # generato da tauri-specta, non va modificato
        ├── lib/i18n/{en,it}.ts
        ├── lib/stores/         # connessioni e sessioni (runes)
        └── components/         # Sidebar, ConnectionForm, TerminalTab,
                                # HostKeyDialog, SecretPrompt, ForwardPanel,
                                # Settings
```

**Confini tra i moduli**

- `pylentra-core` espone un'API async (`ConnectionStore`, `SessionManager`,
  `ForwardManager`). Riceve dall'esterno i percorsi e i canali di output, non
  conosce Tauri e si testa con `cargo test`.
- `src-tauri` tiene il core nello stato di Tauri. Ogni comando inoltra al core
  e converte `CoreError` in `ApiError`. Usa `tauri-plugin-single-instance`,
  `tauri-plugin-updater` e il path resolver di Tauri per le cartelle di
  configurazione e di log.
- Solo `api.ts` chiama `invoke`; i componenti parlano con `api.ts`.
- I segreti passano dalla UI al core una sola volta (al salvataggio o in
  risposta a un prompt). Il core non li restituisce mai alla UI.

## 5. Modello dati

```rust
struct Connection {
    id: Uuid,
    name: String,
    group_id: Option<Uuid>,
    host: String,
    port: u16,                    // default 22
    username: String,
    auth: AuthMethod,
    jump_via: Option<Uuid>,       // un'altra Connection salvata
    forwards: Vec<ForwardRule>,
}

enum AuthMethod {
    Password { save: bool },
    Key { path: PathBuf, save_passphrase: bool },
    Agent,
}

enum ForwardRule {
    Local  { bind: SocketAddr, dest_host: String, dest_port: u16, auto_start: bool },
    Remote { bind_port: u16,   dest_host: String, dest_port: u16, auto_start: bool },
}

struct Group { id: Uuid, name: String }
```

**Validazione nel core, al salvataggio**

- `name`, `host` e `username` non vuoti; `port` compreso tra 1 e 65535.
- `jump_via` deve esistere e non deve formare cicli; la catena può avere al
  massimo 5 livelli.
- Se il file della chiave non esiste si genera un avviso, non un errore:
  potrebbe trovarsi su un disco rimovibile.

Cancellare un gruppo sposta le sue connessioni in "Senza gruppo". Cancellare
una connessione usata come `jump_via` da altre richiede conferma e svuota quei
riferimenti.

## 6. Persistenza

**Posizione dei file**

La cartella di configurazione è quella che Tauri ricava dall'identifier:

- Linux: `~/.config/me.luucka.pylentra/`
- Windows: `%APPDATA%\me.luucka.pylentra\`

| File | Contenuto |
|---|---|
| `connections.toml` | `version = 1`, gruppi e connessioni |
| `settings.toml` | tema, font, dimensione del font, lingua |
| `known_hosts` | host key accettate in Pylentra, formato OpenSSH |

**Regole**

- **Scrittura atomica:** si scrive `<file>.tmp` e poi lo si rinomina.
- **File illeggibile:** viene rinominato in `<file>.corrupt-<timestamp>` e
  l'app parte vuota, con un banner persistente che mostra il percorso. Il
  file non viene mai sovrascritto.
- **Un solo processo scrittore**, grazie a `tauri-plugin-single-instance`.

**Segreti**

- Servizio keychain `me.luucka.pylentra`, account `<uuid>/password` oppure
  `<uuid>/passphrase`. Il TOML contiene solo i flag `save` e
  `save_passphrase`.
- Un segreto inserito a un prompt viene salvato solo dopo un'autenticazione
  riuscita e solo se l'utente ha scelto "salva".
- Cancellare una connessione cancella anche i suoi segreti.
- Keychain non disponibile (ad esempio Linux senza Secret Service): il
  salvataggio fallisce in modo esplicito e i segreti vengono chiesti a ogni
  connessione. Non c'è mai un ripiego in chiaro.
- Nel core i segreti sono `secrecy::SecretString`.

**Known hosts**

- Si leggono il file di Pylentra e `~/.ssh/known_hosts`, quest'ultimo solo in
  lettura. Sono supportati gli host con hash (`|1|…`) e quelli con porta
  (`[host]:port`).
- Le nuove chiavi accettate vengono scritte solo nel file di Pylentra. Non si
  scrive mai in `~/.ssh`.

## 7. Ciclo di vita di una sessione

**Comandi**

- `session_open(connectionId, data: Channel<Raw>, events: Channel<SessionEvent>) -> SessionId`
- `session_write(sessionId, bytes)`
- `session_resize(sessionId, cols, rows)`
- `session_answer(sessionId, answer)`
- `session_close(sessionId)`

**Eventi**

- `data` trasporta byte grezzi (`InvokeResponseBody::Raw`), che arrivano in JS
  come `ArrayBuffer` e vanno passati a `term.write(new Uint8Array(buf))`.
- `events` trasporta JSON:

```rust
enum SessionEvent {
    Status { stage: Connecting | Authenticating | Ready },
    HostKeyPrompt { host, algo, fingerprint_sha256 },
    HostKeyMismatch { host, expected, got, source: Pylentra | OpenSsh },
    SecretPrompt { kind: Password | Passphrase | KeyboardInteractive { prompts } },
    Closed { reason: Exited(code) | Error(ApiError) | UserClosed },
}
```

**Fasi**

1. **Apertura.** La UI crea la scheda nello stato "Connessione…" e chiama
   `session_open`. Rust restituisce subito il `SessionId` e lancia un task
   tokio per la sessione.
2. **Trasporto.** Il core risolve la catena dei jump host. Il primo hop è una
   connessione TCP; i successivi passano per `channel_open_direct_tcpip` sulla
   sessione precedente. Ogni hop ha un timeout di connessione di 15 s.
3. **Host key.** Il controllo avviene in `check_server_key`, hop per hop:
   - chiave nota: si prosegue;
   - chiave sconosciuta: `HostKeyPrompt`, poi si attende la risposta; se
     accettata, viene scritta nel `known_hosts` di Pylentra;
   - chiave diversa: la connessione viene rifiutata con `HostKeyMismatch`. Se
     la chiave viene dal file di Pylentra, la UI offre "Dimentica host"; se
     viene da OpenSSH, mostra `ssh-keygen -R <host>`. Non esiste un "connetti
     comunque".
4. **Autenticazione.**
   - **Password:** viene presa dal keychain, altrimenti con `SecretPrompt`. Si
     prova il metodo `password` e, se il server lo richiede, si ripiega su
     `keyboard-interactive`.
   - **Chiave:** si legge il file; se è cifrata, la passphrase viene dal
     keychain oppure da un prompt. Con una passphrase errata la richiesta si
     ripete, fino a 3 tentativi.
   - **Agent:** `SSH_AUTH_SOCK` su Linux; su Windows la named pipe di OpenSSH
     e Pageant.
5. **Shell.** Si chiede `request_pty("xterm-256color", cols, rows)` e poi
   `request_shell`. Si avviano i forward con `auto_start` e si emette
   `Status::Ready`.
6. **Loop di I/O.** Un `tokio::select!` gestisce due direzioni:
   - dal server: stdout e stderr vanno su `data`; exit, EOF o chiusura del
     canale producono `Closed`;
   - dalla UI: i comandi arrivano da un `mpsc` limitato; `term.onData` produce
     `session_write`, mentre `ResizeObserver` e FitAddon (con debounce di
     50 ms) producono `session_resize` e quindi `window_change`.

   Il keepalive parte ogni 30 s; dopo 3 risposte mancate si emette
   `Closed::Error`.
7. **Chiusura.**
   - L'utente chiude la scheda: `session_close` annulla il task, invia il
     disconnect e ferma i forward della sessione.
   - Chiusura remota: la scheda mostra "Sessione terminata. Premi Invio per
     riconnetterti" e Invio apre una nuova sessione nella stessa scheda.
   - Uscita dall'app: tutte le sessioni vengono chiuse, con un timeout di 2 s.

**Prompt in sospeso**

Ogni attesa di risposta dalla UI è un `oneshot` registrato nel
`SessionManager` e risolto da `session_answer`. Chiudere la scheda elimina il
`oneshot`, e il task termina in modo pulito.

**Forward a runtime**

`ForwardPanel` avvia e ferma i tunnel di una sessione attiva, tramite i
comandi `forward_start` e `forward_stop`. Un tunnel vive quanto la connessione
SSH che lo porta. Un errore di un forward, ad esempio una porta già occupata,
non chiude la sessione.

## 8. Errori

`CoreError`, definito con `thiserror`, ha queste categorie: `Network`,
`HostKey`, `Auth`, `Secret`, `Forward`, `Config`, `Io`. `src-tauri` lo
converte in:

```ts
type ApiError = { code: string; params: Record<string, string | number> };
// es. { code: "auth.all_failed", params: { tried: "publickey,password" } }
```

La UI traduce `code` nella lingua scelta.

**Dove compare ogni errore**

| Origine | Presentazione |
|---|---|
| Sessione (rete, autenticazione, host key) | Riquadro dentro la scheda, con "Riprova" e "Modifica connessione" |
| Validazione del form | Messaggio sotto il campo |
| Forward | Sulla riga del tunnel in `ForwardPanel` |
| Store e keychain | Toast; per un file corrotto all'avvio, un banner persistente |

**Regole**

- Nessun `unwrap` o `panic` nel codice del core, fuori dai test.
- I log usano `tracing` su file con rotazione nella cartella di log dell'app,
  a livello `info`. Non contengono mai segreti né i dati del terminale.
- I `Debug` dei tipi che contengono segreti sono oscurati.

## 9. UI

- **Layout.** Sidebar a sinistra con ricerca, gruppi comprimibili, pallino ●
  per le sessioni aperte, pulsanti "+ Connessione" e impostazioni. A destra
  la barra delle schede, il terminale e `ForwardPanel` in basso.
- **Sidebar.** Doppio clic o Invio apre la connessione. Il menu contestuale
  offre Modifica, Duplica, Sposta in gruppo ed Elimina (con conferma).
- **Form della connessione.** Finestra modale con quattro sezioni: Generale,
  Autenticazione, Jump host e Forward.
- **Prompt per host key e segreti.** Sono legati alla singola scheda, non
  modali globali.
- **Scorciatoie.**
  - `Ctrl+Shift+C` e `Ctrl+Shift+V`: copia e incolla, gestiti esplicitamente.
  - `Ctrl+Shift+W`: chiude la scheda.
  - `Ctrl+Shift+F`: ricerca.
  - `Ctrl+Tab`: scheda successiva.
- **i18n.** Inglese e italiano. La lingua predefinita è quella di sistema, con
  l'inglese come ripiego.
- **Accessibilità.** Sidebar, form e dialoghi sono navigabili da tastiera e
  hanno etichette ARIA.

## 10. Test

**Core**

- Unit test su:
  - validazione del modello;
  - store: lettura e scrittura, scrittura atomica, file corrotto, campo
    `version`;
  - parsing di `known_hosts` (host con hash e con porta);
  - catena dei jump host e cicli;
  - conversione da `CoreError` ad `ApiError`.
- `SecretStore`: implementazione in memoria per i test; test `#[ignore]` per
  `KeyringStore`, da eseguire a mano su ciascuna piattaforma.
- Integrazione con un server `russh` avviato nel test: password, chiave, prompt
  e mismatch della host key, echo sulla PTY, resize, chiusura. Gira su Linux e
  Windows.
- Compatibilità con OpenSSH reale (solo Linux, container `openssh-server`, due
  istanze): keyboard-interactive, catena di jump host, forward locali e
  remoti.

**Frontend**

- Vitest su `api.ts`, sugli store e sull'i18n, incluso un test che verifica
  che le chiavi inglesi e italiane coincidano.
- `svelte-check`.

**Prima di ogni release**

Checklist manuale in `docs/release-checklist.md`, da eseguire su Linux e
Windows.

## 11. CI (GitHub Actions, su ogni PR)

| Job | Contenuto |
|---|---|
| `rust` (Ubuntu e Windows) | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` |
| `audit` | `cargo deny` (vulnerabilità e licenze) |
| `ui` | `pnpm lint`, `svelte-check`, `vitest`, controllo dei binding generati |
| `openssh-compat` (Ubuntu) | test di integrazione con OpenSSH in container |

## 12. Release

- **Versioni.** SemVer. La versione sta solo nel `Cargo.toml` del workspace
  (`tauri.conf.json` la eredita); le modifiche vanno in `CHANGELOG.md`.
- **Build.** Un tag `v*` avvia `tauri-apps/tauri-action`, che crea una GitHub
  Release in bozza con:
  - **Linux:** AppImage e `.deb`, compilati su `ubuntu-22.04`;
  - **Windows:** installer NSIS, installabile per utente senza permessi di
    amministratore;
  - un checksum SHA-256 per ogni artefatto.
- **Aggiornamenti.** `tauri-plugin-updater` usa come endpoint il `latest.json`
  delle GitHub Release, con aggiornamenti firmati da una chiave Tauri. La
  chiave privata sta nei secret di GitHub. Funziona con AppImage e NSIS; chi
  usa il `.deb` aggiorna a mano.
- **Firma del codice.** Su Windows la v1 esce senza firma; l'avviso di
  SmartScreen va documentato nel README.
- **Licenza.** `LICENSE-MIT` e `LICENSE-APACHE` nella root;
  `license = "MIT OR Apache-2.0"` nei `Cargo.toml`.

## 13. Da verificare all'inizio del piano

1. **Agent su Windows.** Supporto di `russh` a Pageant e alla named pipe di
   OpenSSH. Se manca, nella v1 su Windows l'agent copre solo la named pipe
   OpenSSH e la UI lo indica.
2. **Binding dei tipi.** Stato di `tauri-specta` con Tauri 2. Se non è
   stabile, si usa `ts-rs` per i tipi e si scrivono a mano gli stub di
   `api.ts`.
3. **Byte grezzi sul Channel.** Verificare che un `Channel` con
   `InvokeResponseBody::Raw` arrivi davvero come `ArrayBuffer` in JS. In caso
   contrario, si invia il buffer codificato in base64 sul canale `events`.
