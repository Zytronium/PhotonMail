use crate::theme::Rgb;

pub struct Account { pub name: &'static str, pub addr: &'static str, pub color: Rgb, pub sync: &'static str }
pub struct Mail {
    pub account: usize,
    pub folder: usize,
    pub unread: bool,
    pub from: &'static str,
    pub from_email: &'static str,
    pub subject: &'static str,
    pub preview: &'static str,
    pub date: &'static str,
    pub ts: u32, // for sorting; higher = newer
}

pub const ACCOUNTS: [Account; 3] = [
    Account { name: "Personal", addr: "foogle_user@femail.com", color: (109, 74, 255), sync: "2 min ago" },
    Account { name: "Work", addr: "why.wont@it.work", color: (0, 150, 136), sync: "just now" },
    Account { name: "Dev", addr: "still@in.dev", color: (240, 140, 40), sync: "1 hour ago" },
];
pub const FOLDERS: [&str; 6] = ["Inbox", "Drafts", "Sent", "Archive", "Spam", "Trash"];
pub const LABELS: [(&str, Rgb); 3] = [
    ("Receipts", (240, 140, 40)),
    ("Travel", (0, 150, 136)),
    ("Important", (220, 70, 90)),
];
pub const MAILS: [Mail; 8] = [
    Mail { account: 0, folder: 0, unread: true, from: "Alice", from_email: "im.a@femail.com", subject: "Weekend plans", preview: "Are we still on for Saturday?", date: "10:42", ts: 80 },
    Mail { account: 1, folder: 0, unread: true, from: "Build Bot", from_email: "notifications@buildci.io", subject: "CI failed on main", preview: "Job #482 failed at step 3...", date: "09:15", ts: 70 },
    Mail { account: 2, folder: 0, unread: false, from: "Vercel", from_email: "ship@vercel.com", subject: "Deployment ready", preview: "Your project was deployed.", date: "Yesterday", ts: 60 },
    Mail { account: 0, folder: 0, unread: true, from: "Bob", from_email: "bob.henderson@femail.com", subject: "Invoice", preview: "Attached is the invoice for...", date: "Yesterday", ts: 55 },
    Mail { account: 1, folder: 0, unread: false, from: "HR", from_email: "hr@acmecorp.com", subject: "Benefits update", preview: "Open enrollment starts...", date: "Nov 12", ts: 40 },
    Mail { account: 0, folder: 2, unread: false, from: "To: Alice", from_email: "foogle_user@femail.com", subject: "Re: Weekend plans", preview: "Saturday works for me.", date: "Nov 11", ts: 35 },
    Mail { account: 1, folder: 1, unread: false, from: "Draft", from_email: "why.wont@it.work", subject: "Q4 report", preview: "Hi team, attached is...", date: "Nov 10", ts: 30 },
    Mail { account: 2, folder: 3, unread: false, from: "GitHub", from_email: "security-noreply@github.com", subject: "Security advisory", preview: "A dependency in your repo...", date: "Nov 8", ts: 20 },
];
