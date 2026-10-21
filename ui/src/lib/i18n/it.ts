import type { MessageKey } from "./en";

export const it: Record<MessageKey, string> = {
  "app.title": "Pylentra",

  "error.auth.agent_unavailable": "Agente SSH non disponibile: {reason}",
  "error.auth.all_failed": "Autenticazione fallita (provati: {tried}).",
  "error.auth.key_unreadable": "Impossibile leggere la chiave {path}: {reason}",
  "error.cancelled": "Annullato.",
  "error.config.invalid": "Valore non valido per {field}: {reason}",
  "error.config.jump_cycle": "La catena di jump host contiene un ciclo.",
  "error.config.jump_too_deep":
    "La catena di jump host è troppo lunga (massimo 5).",
  "error.config.read_only":
    "La configurazione è stata scritta da una versione più recente ed è in sola lettura.",
  "error.config.unsupported_version":
    "Versione di configurazione {found} non supportata.",
  "error.config.recovered_corrupt":
    "Il file di configurazione era illeggibile. È stato conservato come {backup} e ne è stato creato uno nuovo.",
  "error.forward.permission_denied": "Permesso negato per la porta {port}.",
  "error.forward.port_in_use": "La porta {port} è già in uso.",
  "error.forward.rejected": "Il server ha rifiutato l'inoltro di porta.",
  "error.hostkey.mismatch":
    "La chiave host di {host}:{port} è cambiata. La connessione è stata rifiutata.",
  "error.hostkey.rejected": "Chiave host rifiutata.",
  "error.io": "Errore di I/O: {reason}",
  "error.network.dns": "Impossibile risolvere l'host {host}.",
  "error.network.refused": "Connessione rifiutata da {host}:{port}.",
  "error.network.timeout": "Timeout della connessione a {host}:{port}.",
  "error.network.unreachable": "{host}:{port} non raggiungibile: {reason}",
  "error.not_found": "Non trovato: {what}",
  "error.secret.unavailable": "Portachiavi non disponibile: {reason}",
  "error.ssh.protocol": "Errore SSH: {reason}",
  "error.unknown": "Qualcosa è andato storto.",
};
