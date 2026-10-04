use serde::Deserialize;

use crate::util::esc;

#[derive(Deserialize)]
pub struct Applicant {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub ip: String,
    pub agent: String,
    pub language: String,
    pub origin: String,
    pub created_at: String,
}

impl Applicant {
    fn rows(&self) -> [(&'static str, &str); 7] {
        [
            ("Name", &self.name),
            ("Email", &self.email),
            ("IP address", &self.ip),
            ("Browser", &self.agent),
            ("Language", &self.language),
            ("Signed up from", &self.origin),
            ("Registered at", &self.created_at),
        ]
    }
}

const LOGO: &str = r#"<svg viewBox="0 0 64 64" width="22" height="22" fill="none" stroke="currentColor" stroke-linejoin="round"><path d="M24 55H15a4 4 0 0 1-4-4V13a4 4 0 0 1 4-4h21l12 12v5a11 11 0 0 1-11 11h-4l17.5 18.5" stroke-width="7"/><path d="M36 9v8a4 4 0 0 0 4 4h8z" fill="currentColor" stroke-width="2"/><path d="M20 19h8M20 26h11M20 33h5" stroke-width="5" stroke-linecap="round"/></svg>"#;

const STYLE: &str = "
:root{--bg:#fafafa;--card:#fff;--fg:#0a0a0a;--muted:#737373;--border:#e5e5e5;--primary:#171717;--primary-fg:#fafafa;--danger:#dc2626;--soft:#f5f5f5}
@media (prefers-color-scheme:dark){:root{--bg:#0a0a0a;--card:#171717;--fg:#fafafa;--muted:#a3a3a3;--border:#262626;--primary:#e5e5e5;--primary-fg:#171717;--danger:#f87171;--soft:#262626}}
*{box-sizing:border-box}body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px;
background:var(--bg);color:var(--fg);font:14px/1.5 Inter,ui-sans-serif,system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;-webkit-font-smoothing:antialiased}
.card{width:100%;max-width:520px;background:var(--card);border:1px solid var(--border);border-radius:14px;padding:28px;box-shadow:0 1px 3px rgba(0,0,0,.06);animation:in .3s ease-out}
@keyframes in{from{opacity:0;transform:translateY(8px)}}
.brand{display:flex;align-items:center;gap:10px;font-weight:600;margin-bottom:22px}
.tile{display:flex;align-items:center;justify-content:center;width:34px;height:34px;border-radius:9px;background:var(--primary);color:var(--primary-fg)}
h1{font-size:20px;font-weight:600;letter-spacing:-.01em;margin:0 0 4px}p{margin:0;color:var(--muted)}
dl{display:grid;grid-template-columns:130px 1fr;margin:22px 0 0;border:1px solid var(--border);border-radius:10px;overflow:hidden}
dt,dd{margin:0;padding:10px 12px;border-bottom:1px solid var(--border)}dt{color:var(--muted);background:var(--soft)}dd{word-break:break-word}
dt:nth-last-child(2),dd:last-child{border-bottom:0}
.actions{display:flex;gap:8px;justify-content:flex-end;margin-top:22px}form{margin:0}
button{height:36px;padding:0 16px;border-radius:8px;font:inherit;font-weight:500;cursor:pointer;transition:opacity .15s,transform .05s}
button:active{transform:scale(.98)}button:hover{opacity:.9}
.primary{background:var(--primary);color:var(--primary-fg);border:1px solid var(--primary)}
.danger{background:transparent;color:var(--danger);border:1px solid var(--border)}
.note{margin-top:14px;font-size:12px}
";

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title} - RustKeep</title><style>{STYLE}</style></head><body><main class=\"card\">\
         <div class=\"brand\"><span class=\"tile\">{LOGO}</span>RustKeep</div>{body}</main></body></html>"
    )
}

fn details(a: &Applicant) -> String {
    a.rows().iter().map(|(k, v)| format!("<dt>{k}</dt><dd>{}</dd>", esc(v))).collect()
}

pub fn review_page(a: &Applicant, approve_action: &str, reject_action: &str) -> String {
    let body = format!(
        "<h1>New account request</h1><p>Review the details below before giving this person access.</p>\
         <dl>{}</dl>\
         <div class=\"actions\">\
         <form method=\"post\" action=\"{}\"><button class=\"danger\">Reject</button></form>\
         <form method=\"post\" action=\"{}\"><button class=\"primary\">Approve</button></form></div>\
         <p class=\"note\">Rejecting deletes the request and blocks this email address from signing up again.</p>",
        details(a),
        esc(reject_action),
        esc(approve_action)
    );
    page("Account request", &body)
}

pub fn message_page(title: &str, text: &str) -> String {
    page(title, &format!("<h1>{}</h1><p>{}</p>", esc(title), esc(text)))
}

pub fn approval_email(a: &Applicant, link: &str, logo_url: &str) -> String {
    let rows: String = a
        .rows()
        .iter()
        .map(|(k, v)| {
            format!(
                "<tr><td style=\"padding:10px 12px;border-bottom:1px solid #e5e5e5;background:#f5f5f5;color:#737373;width:130px;vertical-align:top\">{k}</td>\
                 <td style=\"padding:10px 12px;border-bottom:1px solid #e5e5e5;color:#0a0a0a;word-break:break-word\">{}</td></tr>",
                esc(v)
            )
        })
        .collect();
    format!(
        "<!doctype html><html><body style=\"margin:0;padding:32px 12px;background:#fafafa;font-family:Inter,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:14px;line-height:1.5;color:#0a0a0a\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\"><tr><td align=\"center\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"max-width:520px;background:#ffffff;border:1px solid #e5e5e5;border-radius:14px\">\
         <tr><td style=\"padding:28px 28px 0\"><table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\"><tr>\
         <td><img src=\"{logo}\" width=\"34\" height=\"34\" alt=\"RustKeep\" style=\"display:block;border-radius:9px\"></td>\
         <td style=\"padding-left:10px;font-weight:600\">RustKeep</td></tr></table></td></tr>\
         <tr><td style=\"padding:22px 28px 0\"><div style=\"font-size:20px;font-weight:600\">New account request</div>\
         <div style=\"color:#737373\">{name} wants to join your RustKeep workspace.</div></td></tr>\
         <tr><td style=\"padding:22px 28px 0\"><table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" \
         style=\"border:1px solid #e5e5e5;border-radius:10px;border-collapse:separate;overflow:hidden\">{rows}</table></td></tr>\
         <tr><td style=\"padding:24px 28px 28px\"><a href=\"{link}\" style=\"display:inline-block;background:#171717;color:#fafafa;text-decoration:none;\
         font-weight:500;padding:10px 18px;border-radius:8px\">Review request</a>\
         <div style=\"margin-top:14px;color:#737373;font-size:12px\">You can approve or reject the account on the review page.</div></td></tr>\
         </table></td></tr></table></body></html>",
        logo = esc(logo_url),
        name = esc(&a.name),
        link = esc(link),
    )
}

pub fn notice_email(title: &str, text: &str, action: &str, link: &str, logo_url: &str) -> String {
    format!(
        "<!doctype html><html><body style=\"margin:0;padding:32px 12px;background:#fafafa;font-family:Inter,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:14px;line-height:1.5;color:#0a0a0a\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\"><tr><td align=\"center\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"max-width:520px;background:#ffffff;border:1px solid #e5e5e5;border-radius:14px\">\
         <tr><td style=\"padding:28px 28px 0\"><table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\"><tr>\
         <td><img src=\"{logo}\" width=\"34\" height=\"34\" alt=\"RustKeep\" style=\"display:block;border-radius:9px\"></td>\
         <td style=\"padding-left:10px;font-weight:600\">RustKeep</td></tr></table></td></tr>\
         <tr><td style=\"padding:22px 28px 0\"><div style=\"font-size:20px;font-weight:600\">{title}</div>\
         <div style=\"color:#737373\">{text}</div></td></tr>\
         <tr><td style=\"padding:24px 28px 28px\"><a href=\"{link}\" style=\"display:inline-block;background:#171717;color:#fafafa;text-decoration:none;\
         font-weight:500;padding:10px 18px;border-radius:8px\">{action}</a></td></tr>\
         </table></td></tr></table></body></html>",
        logo = esc(logo_url),
        title = esc(title),
        text = esc(text),
        action = esc(action),
        link = esc(link),
    )
}
