//! SMTP via `lettre` plus optional IMAP Sent-folder append.
//! Port of legacy `services/email_client.py`. Secrets come only from the OS keyring.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;

use lettre::message::{header::ContentType, Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use once_cell::sync::Lazy;

use crate::notify::message;
use crate::notify::{secret_first, setting_flag, setting_or_default};
use crate::storage::logging;

const SENT_FOLDER_HINTS: [&str; 6] = [
    "Sent",
    "Sent Items",
    "Gesendet",
    "Gesendete Objekte",
    "INBOX.Sent",
    "INBOX.Gesendet",
];

static CACHED_SENT_FOLDER: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub sender_addr: String,
    pub sender_name: String,
    pub sandbox: bool,
    pub fallback_recipient: String,
}

#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub configured_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFolder {
    pub flags: Vec<String>,
    pub path: String,
    pub name: String,
    pub delimiter: String,
}

/// Sandbox: rewrite the recipient to the fallback address. `Err` if sandbox has no fallback.
pub fn resolve_recipient(original: &str, sandbox: bool, fallback: &str) -> Result<String, &'static str> {
    if !sandbox {
        return Ok(original.to_string());
    }
    let fallback = fallback.trim();
    if fallback.is_empty() {
        return Err("Sandbox-Modus aktiv, aber kein Fallback-Empfänger konfiguriert.");
    }
    Ok(fallback.to_string())
}

pub fn load_smtp_config() -> SmtpConfig {
    SmtpConfig {
        host: setting_or_default("smtp_host", "").trim().to_string(),
        port: parse_port(&setting_or_default("smtp_port", "587"), 587),
        user: secret_first(&["smtp_user"]).unwrap_or_default(),
        password: secret_first(&["smtp_pass"]).unwrap_or_default(),
        sender_addr: setting_or_default("smtp_sender_addr", "").trim().to_string(),
        sender_name: setting_or_default("smtp_sender_name", "Dropbox Uploader"),
        sandbox: setting_flag("smtp_sandbox_mode"),
        fallback_recipient: setting_or_default("smtp_fallback_recipient", "")
            .trim()
            .to_string(),
    }
}

pub fn load_imap_config() -> ImapConfig {
    let smtp = load_smtp_config();
    let mut host = setting_or_default("imap_host", "").trim().to_string();
    if host.is_empty() {
        host = smtp.host.clone();
    }
    let same = setting_or_default("imap_same_credentials", "true")
        .trim()
        .eq_ignore_ascii_case("true");
    let (user, password) = if same {
        (smtp.user, smtp.password)
    } else {
        (
            secret_first(&["imap_user", "smtp_user"]).unwrap_or_default(),
            secret_first(&["imap_pass", "smtp_pass"]).unwrap_or_default(),
        )
    };
    ImapConfig {
        enabled: setting_or_default("imap_save_sent_enabled", "true")
            .eq_ignore_ascii_case("true"),
        host,
        port: parse_port(&setting_or_default("imap_port", "993"), 993),
        user,
        password,
        configured_folder: setting_or_default("imap_sent_folder", "").trim().to_string(),
    }
}

fn parse_port(raw: &str, default: u16) -> u16 {
    raw.trim().parse().unwrap_or_else(|_| {
        logging::log_error(&format!("Ungültiger SMTP/IMAP-Port '{raw}', verwende {default}."));
        default
    })
}

pub fn is_valid_mailbox_path(path: &str, delimiter: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }
    if path == delimiter {
        return false;
    }
    if !delimiter.is_empty() && path.starts_with(delimiter) {
        return false;
    }
    true
}

pub fn folder_has_sent_flag(folder: &MailFolder) -> bool {
    folder.flags.iter().any(|flag| {
        let upper = flag.to_ascii_uppercase();
        upper == "\\SENT" || upper == "SENT"
    })
}

pub fn folder_matches_sent_hint(folder: &MailFolder) -> bool {
    let path = folder.path.as_str();
    let name = if folder.name.is_empty() {
        path
    } else {
        folder.name.as_str()
    };
    let path_lower = path.to_ascii_lowercase();
    SENT_FOLDER_HINTS
        .iter()
        .any(|hint| path == *hint || name == *hint)
        || path_lower.contains("gesendet")
        || path_lower.contains("sent")
}

/// Choose the Sent mailbox: cache → `\Sent` flag → name hint → configured path.
pub fn resolve_sent_folder_path(
    folders: &[MailFolder],
    cached: Option<&str>,
    configured_folder: &str,
) -> Option<(String, &'static str)> {
    if folders.is_empty() {
        return None;
    }
    let folder_paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();

    if let Some(cached) = cached {
        if is_valid_mailbox_path(cached, "/") && folder_paths.contains(&cached) {
            return Some((cached.to_string(), "cache"));
        }
    }

    if let Some(folder) = folders
        .iter()
        .find(|f| folder_has_sent_flag(f) && is_valid_mailbox_path(&f.path, &f.delimiter))
    {
        return Some((folder.path.clone(), "\\Sent"));
    }

    if let Some(folder) = folders
        .iter()
        .find(|f| folder_matches_sent_hint(f) && is_valid_mailbox_path(&f.path, &f.delimiter))
    {
        return Some((folder.path.clone(), "name"));
    }

    let configured = configured_folder.trim();
    if !configured.is_empty() && folder_paths.contains(&configured) {
        return Some((configured.to_string(), "configured"));
    }

    None
}

pub async fn send_email(to_recipient: &str, subject: &str, body: &str) -> bool {
    let cfg = load_smtp_config();
    let to_recipient = match resolve_recipient(to_recipient, cfg.sandbox, &cfg.fallback_recipient) {
        Ok(addr) => {
            if cfg.sandbox && addr != to_recipient {
                logging::log_info(&format!(
                    "Sandbox-Modus: E-Mail für {to_recipient} wird an Fallback {addr} gesendet."
                ));
            }
            addr
        }
        Err(msg) => {
            logging::log_error(&format!("E-Mail-Versand fehlgeschlagen: {msg}"));
            return false;
        }
    };

    if cfg.host.is_empty() || cfg.user.is_empty() || cfg.password.is_empty() || cfg.sender_addr.is_empty()
    {
        logging::log_error("E-Mail-Versand fehlgeschlagen: SMTP-Einstellungen unvollständig.");
        return false;
    }

    let from = match parse_mailbox(&cfg.sender_name, &cfg.sender_addr) {
        Some(m) => m,
        None => {
            logging::log_error("E-Mail-Versand fehlgeschlagen: Ungültige Absender-Adresse.");
            return false;
        }
    };
    let to = match to_recipient.parse::<Mailbox>() {
        Ok(m) => m,
        Err(_) => {
            logging::log_error(&format!(
                "E-Mail-Versand fehlgeschlagen: Ungültiger Empfänger '{to_recipient}'."
            ));
            return false;
        }
    };

    let message = match Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body.to_string())
    {
        Ok(m) => m,
        Err(e) => {
            logging::log_error(&format!("Allgemeiner Fehler beim E-Mail-Versand: {e}"));
            return false;
        }
    };
    let raw = message.formatted();

    logging::log_info(&format!("Versende E-Mail an {to_recipient}..."));

    let creds = Credentials::new(cfg.user.clone(), cfg.password.clone());
    let mailer = match AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host) {
        Ok(builder) => builder.port(cfg.port).credentials(creds).build(),
        Err(e) => {
            logging::log_error(&format!("SMTP-Fehler beim Senden der E-Mail: {e}"));
            return false;
        }
    };

    match mailer.send(message).await {
        Ok(_) => {
            logging::log_info(&format!("E-Mail an {to_recipient} erfolgreich versendet."));
            save_to_sent_folder(raw).await;
            true
        }
        Err(e) => {
            logging::log_error(&format!("SMTP-Fehler beim Senden der E-Mail: {e}"));
            false
        }
    }
}

fn parse_mailbox(name: &str, addr: &str) -> Option<Mailbox> {
    let address = addr.trim().parse().ok()?;
    let display = name.trim();
    if display.is_empty() {
        Some(Mailbox::new(None, address))
    } else {
        Some(Mailbox::new(Some(display.to_string()), address))
    }
}

pub async fn send_upload_success_email(
    directory_name: &str,
    share_link: &str,
    email: Option<&str>,
    vorname: Option<&str>,
) -> bool {
    let recipient = email
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let fallback = setting_or_default("smtp_fallback_recipient", "");
            let trimmed = fallback.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    let Some(recipient) = recipient else {
        logging::log_warn("Kein Fallback-Empfänger für Erfolgs-Mail konfiguriert.");
        return false;
    };
    let subject = message::upload_success_email_subject(directory_name);
    let body = message::upload_success_email_html(message::display_name(vorname), share_link);
    send_email(&recipient, &subject, &body).await
}

#[allow(dead_code)]
pub async fn send_upload_failure_email(directory_name: &str, error: &str) -> bool {
    let fallback = setting_or_default("smtp_fallback_recipient", "");
    let recipient = fallback.trim();
    if recipient.is_empty() {
        logging::log_warn("Kein Fallback-Empfänger für Fehler-Mail konfiguriert.");
        return false;
    }
    let subject = message::upload_failure_email_subject(directory_name);
    let body = message::upload_failure_email_body(directory_name, error);
    send_email(recipient, &subject, &body).await
}

async fn save_to_sent_folder(raw_message: Vec<u8>) {
    let cfg = load_imap_config();
    if !cfg.enabled {
        return;
    }
    if cfg.host.is_empty() || cfg.user.is_empty() || cfg.password.is_empty() {
        logging::log_warn("IMAP-Ablage übersprungen: Zugangsdaten unvollständig.");
        return;
    }
    let result = tokio::task::spawn_blocking(move || save_to_sent_folder_blocking(cfg, raw_message)).await;
    if let Err(e) = result {
        logging::log_warn(&format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"));
    }
}

fn save_to_sent_folder_blocking(cfg: ImapConfig, raw_message: Vec<u8>) {
    match imap_append_sent(&cfg, &raw_message) {
        Ok((folder, source)) => {
            if let Ok(mut guard) = CACHED_SENT_FOLDER.lock() {
                *guard = Some(folder.clone());
            }
            logging::log_info(&format!(
                "E-Mail in IMAP-Ordner '{folder}' abgelegt (Erkennung: {source})."
            ));
        }
        Err(e) => logging::log_warn(&e),
    }
}

fn imap_append_sent(cfg: &ImapConfig, raw_message: &[u8]) -> Result<(String, &'static str), String> {
    let mut session = imap_login(&cfg.host, cfg.port, &cfg.user, &cfg.password)?;
    let folders = list_mail_folders(&mut session);
    if folders.is_empty() {
        let _ = session.logout();
        return Err("IMAP-Ablage übersprungen: Kein Gesendet-Ordner auf dem Server gefunden. Verfügbare Ordner: (keine)".into());
    }

    let cached = CACHED_SENT_FOLDER
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .filter(|p| is_valid_mailbox_path(p, "/"));

    let Some((sent_folder, source)) =
        resolve_sent_folder_path(&folders, cached.as_deref(), &cfg.configured_folder)
    else {
        let available = folders
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let _ = session.logout();
        return Err(format!(
            "IMAP-Ablage übersprungen: Kein Gesendet-Ordner auf dem Server gefunden. Verfügbare Ordner: {available}"
        ));
    };

    if !is_valid_mailbox_path(&sent_folder, "/") {
        let _ = session.logout();
        return Err(format!(
            "IMAP-Ablage übersprungen: Ungültiger Gesendet-Ordner '{sent_folder}'."
        ));
    }

    let append = session.append_with_flags(
        &sent_folder,
        raw_message,
        &[imap::types::Flag::Seen],
    );
    let _ = session.logout();
    match append {
        Ok(()) => Ok((sent_folder, source)),
        Err(e) => Err(format!(
            "IMAP-Ablage fehlgeschlagen (SMTP war OK) für '{sent_folder}': {e}"
        )),
    }
}

fn imap_login(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<imap::Session<native_tls::TlsStream<TcpStream>>, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"))?
        .next()
        .ok_or_else(|| "IMAP-Ablage fehlgeschlagen (SMTP war OK): Host nicht auflösbar".to_string())?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"))?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(20)));
    let tls = native_tls::TlsConnector::new()
        .map_err(|e| format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"))?;
    let tls_stream = tls
        .connect(host, tcp)
        .map_err(|e| format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"))?;
    let client = imap::Client::new(tls_stream);
    client
        .login(user, password)
        .map_err(|(e, _)| format!("IMAP-Ablage fehlgeschlagen (SMTP war OK): {e}"))
}

fn list_mail_folders(
    session: &mut imap::Session<native_tls::TlsStream<TcpStream>>,
) -> Vec<MailFolder> {
    match session.list(None, Some("*")) {
        Ok(names) => names
            .iter()
            .filter_map(|name| {
                let delimiter = name.delimiter().unwrap_or("/").to_string();
                let path = name.name().to_string();
                if !is_valid_mailbox_path(&path, &delimiter) {
                    return None;
                }
                let folder_name = if delimiter.is_empty() {
                    path.clone()
                } else {
                    path.rsplit(&delimiter)
                        .next()
                        .unwrap_or(&path)
                        .to_string()
                };
                let flags = name
                    .attributes()
                    .iter()
                    .map(flag_to_string)
                    .collect();
                Some(MailFolder {
                    flags,
                    path,
                    name: folder_name,
                    delimiter,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn flag_to_string(attr: &imap::types::NameAttribute<'_>) -> String {
    match attr {
        imap::types::NameAttribute::NoInferiors => "\\Noinferiors".into(),
        imap::types::NameAttribute::NoSelect => "\\Noselect".into(),
        imap::types::NameAttribute::Marked => "\\Marked".into(),
        imap::types::NameAttribute::Unmarked => "\\Unmarked".into(),
        imap::types::NameAttribute::Custom(s) => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, flags: &[&str]) -> MailFolder {
        let delimiter = if path.contains('.') { "." } else { "/" };
        let name = path.rsplit(['/', '.']).next().unwrap_or(path).to_string();
        MailFolder {
            flags: flags.iter().map(|s| (*s).to_string()).collect(),
            path: path.to_string(),
            name,
            delimiter: delimiter.to_string(),
        }
    }

    #[test]
    fn sandbox_rewrites_recipient() {
        assert_eq!(
            resolve_recipient("kunde@ex.de", true, "dev@ex.de").unwrap(),
            "dev@ex.de"
        );
        assert_eq!(
            resolve_recipient("kunde@ex.de", false, "dev@ex.de").unwrap(),
            "kunde@ex.de"
        );
        assert!(resolve_recipient("kunde@ex.de", true, "").is_err());
        assert!(resolve_recipient("kunde@ex.de", true, "  ").is_err());
    }

    #[test]
    fn mailbox_path_rejects_empty_and_leading_delimiter() {
        assert!(!is_valid_mailbox_path("", "/"));
        assert!(!is_valid_mailbox_path("/", "/"));
        assert!(!is_valid_mailbox_path("/Sent", "/"));
        assert!(is_valid_mailbox_path("Sent", "/"));
        assert!(is_valid_mailbox_path("INBOX.Sent", "."));
    }

    #[test]
    fn sent_folder_prefers_flag_then_hint_then_configured() {
        let folders = vec![
            folder("INBOX", &[]),
            folder("Archive", &[]),
            folder("Gesendete Objekte", &[]),
        ];
        let (path, source) = resolve_sent_folder_path(&folders, None, "Archive").unwrap();
        assert_eq!(path, "Gesendete Objekte");
        assert_eq!(source, "name");

        let folders = vec![
            folder("INBOX", &[]),
            folder("Sent", &["\\Sent"]),
            folder("Gesendet", &[]),
        ];
        let (path, source) = resolve_sent_folder_path(&folders, None, "").unwrap();
        assert_eq!(path, "Sent");
        assert_eq!(source, "\\Sent");

        let folders = vec![folder("INBOX", &[]), folder("Outbox", &[])];
        let (path, source) = resolve_sent_folder_path(&folders, None, "Outbox").unwrap();
        assert_eq!(path, "Outbox");
        assert_eq!(source, "configured");

        let (path, source) =
            resolve_sent_folder_path(&folders, Some("Outbox"), "INBOX").unwrap();
        assert_eq!(path, "Outbox");
        assert_eq!(source, "cache");
    }

    #[test]
    fn sent_folder_none_when_empty() {
        assert!(resolve_sent_folder_path(&[], None, "Sent").is_none());
    }
}
