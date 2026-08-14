//! Shared notification copy (port of legacy `services/message_client.py` templates
//! plus the email/SMS bodies from `email_client.py` / `sms_client.py`).

pub const AERO_PHONE: &str = "05674-99930";

const SUCCESS_EMAIL_HTML: &str = r#"
                <html>
                <head>
                    <style>
                        body { font-family: Arial, sans-serif; line-height: 1.6; }
                        .container { width: 90%; margin: auto; padding: 20px; border: 1px solid #ddd; border-radius: 5px; }
                        .button {
                            background-color: #007bff; color: #ffffff; padding: 10px 15px;
                            text-decoration: none; border-radius: 5px; display: inline-block;
                        }
                        .button:hover { background-color: #0056b3; }
                        .link { color: #007bff; }
                    </style>
                </head>
                <body>
                    <div class="container">
                        <h2>Hallo __VORNAME__,</h2>
                        <p>vielen Dank für deinen Besuch.</p>

                        <p>Die Medien zu deinem Sprung wurden erfolgreich hochgeladen und sind jetzt verfügbar.</p>
                        <p>Du kannst sie über den folgenden Link herunterladen:</p>
                        <p>
                            <a href="__SHARE_LINK__" class="button">Zum Download</a>
                        </p>
                        <p>
                            Falls der Button nicht funktioniert, kopiere bitte diesen Link in deinen Browser:<br>
                            <a href="__SHARE_LINK__" class="link">__SHARE_LINK__</a>
                        </p>

                        <p>Bei Fragen ruf einfach bei uns an unter 05674-99930, montags, dienstags, donnerstags und freitags 9:30 - 13 Uhr.</p>
                        <p>Der Link bleibt ca. 14 Tage aktiv.</p>
                        <p>Dein AERO Fallschirmsport Team</p>
                    </div>
                </body>
                </html>
                "#;

pub fn display_name(value: Option<&str>) -> &str {
    value.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("")
}

pub fn upload_success_email_subject(directory_name: &str) -> String {
    format!("Upload erfolgreich: {directory_name}")
}

pub fn upload_success_email_html(vorname: &str, share_link: &str) -> String {
    SUCCESS_EMAIL_HTML
        .replace("__VORNAME__", vorname)
        .replace("__SHARE_LINK__", share_link)
}

pub fn upload_failure_email_subject(directory_name: &str) -> String {
    format!("Upload FEHLGESCHLAGEN: {directory_name}")
}

pub fn upload_failure_email_body(directory_name: &str, error: &str) -> String {
    format!(
        "Hallo,\n\n\
         Das Verzeichnis '{directory_name}' konnte NICHT hochgeladen werden.\n\n\
         Fehlerdetails:\n\
         {error}\n\n\
         Das Verzeichnis wurde in den Fehler-Ordner verschoben (falls konfiguriert)."
    )
}

/// SMS / WhatsApp body used after a successful upload.
pub fn download_link_text(first_name: &str, share_link: &str) -> String {
    format!(
        "Hallo {first_name},\n\
         dein Medien-Download ist fertig.\n\
         Link (14 Tage gültig): {share_link}\n\n\
         Dein AERO Team\n\
         (Bei Fragen: {AERO_PHONE})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_email_contains_name_and_link() {
        let html = upload_success_email_html("Anna", "https://dropbox.com/s/x");
        assert!(html.contains("Hallo Anna,"));
        assert!(html.contains("https://dropbox.com/s/x"));
        assert!(html.contains("Zum Download"));
        assert_eq!(
            upload_success_email_subject("Job-1"),
            "Upload erfolgreich: Job-1"
        );
    }

    #[test]
    fn sms_text_matches_legacy() {
        let text = download_link_text("Max", "https://ex/a");
        assert!(text.starts_with("Hallo Max,\n"));
        assert!(text.contains("https://ex/a"));
        assert!(text.contains(AERO_PHONE));
        assert!(text.contains("Dein AERO Team"));
    }

    #[test]
    fn failure_email_includes_error() {
        let body = upload_failure_email_body("ordner", "timeout");
        assert!(body.contains("'ordner'"));
        assert!(body.contains("timeout"));
        assert_eq!(
            upload_failure_email_subject("ordner"),
            "Upload FEHLGESCHLAGEN: ordner"
        );
    }

    #[test]
    fn display_name_trims_empty() {
        assert_eq!(display_name(None), "");
        assert_eq!(display_name(Some("  ")), "");
        assert_eq!(display_name(Some(" Anna ")), "Anna");
    }
}
