use crate::theme::Rgb;

pub struct Account { pub name: &'static str, pub addr: &'static str, pub color: Rgb, pub sync: &'static str }

/// Content type of the mail body.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MailContentType {
    #[default]
    PlainText,
    Html,
}

pub struct Mail {
    pub account: usize,
    pub folder: usize,
    pub unread: bool,
    pub from: &'static str,
    pub from_email: &'static str,
    pub subject: &'static str,
    /// Plain-text preview shown in the message list.
    pub preview: &'static str,
    /// Full body content — either plain text or an HTML document fragment.
    pub body: &'static str,
    pub content_type: MailContentType,
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
pub const MAILS: [Mail; 16] = [
    // 1. Simple plain-text email
    Mail {
        account: 0, folder: 0, unread: true,
        from: "Alice", from_email: "im.a@femail.com",
        subject: "Weekend plans",
        preview: "Are we still on for Saturday?",
        body: "Hey!\n\nAre we still on for Saturday? Let me know what time works for you.\n\nBest,\nAlice",
        content_type: MailContentType::PlainText,
        date: "10:42", ts: 90,
    },
    // 2. HTML marketing email with rich formatting
    Mail {
        account: 0, folder: 0, unread: true,
        from: "Foogle Store", from_email: "deals@fooglestore.com",
        subject: "🔥 Black Friday — Up to 60% Off!",
        preview: "Our biggest sale of the year is here...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #f4f4f8; color: #1a1a2e; }
  .container { max-width: 600px; margin: 0 auto; background: #ffffff; }
  .header { background: linear-gradient(135deg, #6d4aff, #ff6d4a); padding: 40px 30px; text-align: center; color: #fff; }
  .header h1 { margin: 0; font-size: 32px; }
  .header p { margin: 8px 0 0; font-size: 16px; opacity: 0.9; }
  .content { padding: 30px; }
  .card { border: 1px solid #e8e8f0; border-radius: 12px; padding: 20px; margin-bottom: 16px; }
  .card h3 { margin: 0 0 8px; color: #6d4aff; }
  .card .price { font-size: 24px; font-weight: bold; color: #1a1a2e; }
  .card .old-price { text-decoration: line-through; color: #999; font-size: 14px; margin-left: 8px; }
  .btn { display: inline-block; background: #6d4aff; color: #fff; padding: 14px 32px; border-radius: 8px; text-decoration: none; font-weight: bold; font-size: 16px; margin: 16px 0; }
  .footer { background: #1a1a2e; color: #aaa; padding: 20px 30px; text-align: center; font-size: 12px; }
  .footer a { color: #6d4aff; }
  .badge { display: inline-block; background: #ff6d4a; color: #fff; padding: 4px 12px; border-radius: 20px; font-size: 12px; font-weight: bold; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h1>🔥 Black Friday Sale</h1>
    <p>Our biggest sale of the year — ends Sunday midnight!</p>
  </div>
  <div class="content">
    <p>Hi there! Don't miss our <strong>Black Friday deals</strong> with up to <span class="badge">60% OFF</span> on everything.</p>

    <div class="card">
      <h3>Wireless Headphones Pro</h3>
      <p>Noise-cancelling, 40-hour battery, premium sound.</p>
      <span class="price">$79.99</span><span class="old-price">$199.99</span>
    </div>

    <div class="card">
      <h3>Smart Watch Ultra</h3>
      <p>Heart rate, GPS, water-resistant to 50m.</p>
      <span class="price">$159.99</span><span class="old-price">$349.99</span>
    </div>

    <div style="text-align: center;">
      <a class="btn" href="#">Shop All Deals →</a>
    </div>

    <p style="font-size: 13px; color: #888;">Use code <code>BLACKFRIDAY</code> for an extra 10% off at checkout.</p>
  </div>
  <div class="footer">
    <p>You received this email because you subscribed to Foogle Store deals.</p>
    <p><a href="#">Unsubscribe</a> · <a href="#">Manage preferences</a></p>
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "10:30", ts: 85,
    },
    // 3. CI/CD notification with code blocks
    Mail {
        account: 1, folder: 0, unread: true,
        from: "Build Bot", from_email: "notifications@buildci.io",
        subject: "CI failed on main",
        preview: "Job #482 failed at step 3...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: "SF Mono", "Fira Code", "Consolas", monospace; margin: 0; padding: 0; background: #1a1a2e; color: #e5e5f5; }
  .container { max-width: 640px; margin: 0 auto; padding: 24px; }
  .status { display: inline-block; padding: 4px 14px; border-radius: 6px; font-weight: bold; font-size: 14px; }
  .status-fail { background: #dc465a; color: #fff; }
  .status-pass { background: #50c878; color: #fff; }
  h1 { font-size: 20px; margin: 0 0 16px; }
  .meta { color: #9696aa; font-size: 13px; margin-bottom: 20px; }
  .step { border: 1px solid #2d2d3a; border-radius: 8px; padding: 14px; margin-bottom: 10px; }
  .step-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
  .step-name { font-weight: bold; }
  pre { background: #12121a; padding: 12px; border-radius: 6px; overflow-x: auto; font-size: 12px; line-height: 1.6; color: #dc465a; margin: 0; }
  .link { color: #6d4aff; text-decoration: none; }
  table { width: 100%; border-collapse: collapse; margin: 16px 0; }
  th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid #2d2d3a; font-size: 13px; }
  th { color: #9696aa; }
</style>
</head>
<body>
<div class="container">
  <h1><span class="status status-fail">FAILED</span> Build #482 — main</h1>
  <div class="meta">Triggered by <a class="link" href="#">commit a3f8c2d</a> · 2 minutes ago · Runner: ubuntu-latest</div>

  <table>
    <tr><th>Step</th><th>Status</th><th>Duration</th></tr>
    <tr><td>Checkout</td><td><span class="status status-pass">PASS</span></td><td>12s</td></tr>
    <tr><td>Install deps</td><td><span class="status status-pass">PASS</span></td><td>48s</td></tr>
    <tr><td>Run tests</td><td><span class="status status-fail">FAIL</span></td><td>34s</td></tr>
    <tr><td>Build release</td><td>—</td><td>—</td></tr>
  </table>

  <div class="step">
    <div class="step-header">
      <span class="step-name">Run tests</span>
      <span class="status status-fail">FAIL</span>
    </div>
    <pre>thread 'tests::test_auth_flow' panicked at src/auth.rs:142:
assertion `left == right` failed
  left: "401 Unauthorized"
 right: "200 OK"

note: run with RUST_BACKTRACE=1 for details</pre>
  </div>

  <p style="margin-top: 20px;">
    <a class="link" href="#">View full logs →</a>
  </p>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "09:15", ts: 80,
    },
    // 4. Receipt / invoice email
    Mail {
        account: 0, folder: 0, unread: true,
        from: "Bob", from_email: "bob.henderson@femail.com",
        subject: "Invoice #2024-0892",
        preview: "Here's the invoice for the Q4 consulting...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: Georgia, "Times New Roman", serif; margin: 0; padding: 0; background: #fafafa; color: #333; }
  .container { max-width: 520px; margin: 0 auto; background: #fff; border: 1px solid #e0e0e0; }
  .header { background: #2d2d3a; color: #fff; padding: 24px 30px; }
  .header h2 { margin: 0; font-size: 18px; }
  .header p { margin: 4px 0 0; opacity: 0.7; font-size: 13px; }
  .content { padding: 30px; }
  .content h3 { margin: 0 0 16px; color: #2d2d3a; }
  .line-item { display: flex; justify-content: space-between; padding: 10px 0; border-bottom: 1px solid #f0f0f0; }
  .line-item:last-of-type { border-bottom: none; }
  .total { display: flex; justify-content: space-between; padding: 16px 0 0; border-top: 2px solid #2d2d3a; font-weight: bold; font-size: 18px; }
  .amount { font-family: "SF Mono", monospace; }
  .note { background: #f8f8ff; border-left: 3px solid #6d4aff; padding: 12px 16px; margin: 20px 0; font-size: 14px; color: #555; }
  .footer { padding: 20px 30px; text-align: center; font-size: 12px; color: #999; border-top: 1px solid #eee; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h2>Bob Henderson Consulting</h2>
    <p>Invoice #2024-0892 · Due Nov 30, 2026</p>
  </div>
  <div class="content">
    <h3>Bill To</h3>
    <p style="margin: 0 0 20px; font-size: 14px;">Acme Corp<br>123 Business Ave<br>San Francisco, CA 94102</p>

    <div class="line-item">
      <span>Backend API Development (40 hrs)</span>
      <span class="amount">$8,000.00</span>
    </div>
    <div class="line-item">
      <span>Database Optimization (12 hrs)</span>
      <span class="amount">$2,400.00</span>
    </div>
    <div class="line-item">
      <span>Code Review &amp; Mentoring (8 hrs)</span>
      <span class="amount">$1,600.00</span>
    </div>

    <div class="total">
      <span>Total Due</span>
      <span class="amount">$12,000.00</span>
    </div>

    <div class="note">
      Payment via bank transfer. Please reference invoice #2024-0892.
    </div>
  </div>
  <div class="footer">
    Bob Henderson · bob.henderson@femail.com · (555) 123-4567
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Yesterday", ts: 75,
    },
    // 5. Newsletter with images and columns
    Mail {
        account: 2, folder: 0, unread: false,
        from: "Dev Weekly", from_email: "newsletter@devweekly.io",
        subject: "Dev Weekly #312: Rust 2024 Edition",
        preview: "This week: Rust 2024, new CSS features...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #0f0f17; color: #e5e5f5; }
  .container { max-width: 580px; margin: 0 auto; }
  .hero { background: linear-gradient(135deg, #f08c28, #6d4aff); padding: 48px 30px; text-align: center; }
  .hero h1 { margin: 0; font-size: 28px; color: #fff; }
  .hero .issue { color: rgba(255,255,255,0.8); font-size: 14px; margin-top: 6px; }
  .content { padding: 30px; }
  .article { padding: 20px 0; border-bottom: 1px solid #2d2d3a; }
  .article:last-child { border-bottom: none; }
  .article .tag { display: inline-block; background: #2d2d3a; color: #f08c28; padding: 2px 10px; border-radius: 4px; font-size: 11px; font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px; }
  .article h3 { margin: 8px 0 4px; font-size: 17px; }
  .article h3 a { color: #e5e5f5; text-decoration: none; }
  .article p { margin: 0; color: #9696aa; font-size: 14px; line-height: 1.6; }
  .columns { display: flex; gap: 16px; }
  .column { flex: 1; background: #1a1a2e; border-radius: 10px; padding: 16px; }
  .column h4 { margin: 0 0 8px; font-size: 14px; color: #6d4aff; }
  .column ul { margin: 0; padding-left: 16px; font-size: 13px; color: #9696aa; }
  .column li { margin-bottom: 4px; }
  .footer { padding: 24px 30px; text-align: center; font-size: 12px; color: #555; }
  .footer a { color: #6d4aff; text-decoration: none; }
</style>
</head>
<body>
<div class="container">
  <div class="hero">
    <h1>🦀 Dev Weekly</h1>
    <div class="issue">Issue #312 · November 2026</div>
  </div>
  <div class="content">
    <div class="article">
      <span class="tag">Rust</span>
      <h3><a href="#">Rust 2024 Edition is Here</a></h3>
      <p>The new edition brings inline const, generic associated types improvements, and stricter borrow checking defaults.</p>
    </div>
    <div class="article">
      <span class="tag">CSS</span>
      <h3><a href="#">Container Queries in All Browsers</a></h3>
      <p>After years of development, CSS container queries are now universally supported. Here's how to use them.</p>
    </div>
    <div class="article">
      <span class="tag">AI</span>
      <h3><a href="#">Building Local AI Agents</a></h3>
      <p>A deep dive into running small language models on your own hardware for privacy-first applications.</p>
    </div>

    <div class="columns">
      <div class="column">
        <h4>📦 Packages</h4>
        <ul>
          <li>Tokio 1.42</li>
          <li>Actix Web 4.6</li>
          <li>Leptos 0.7</li>
        </ul>
      </div>
      <div class="column">
        <h4>📚 Reading</h4>
        <ul>
          <li>Designing Data-Intensive Apps</li>
          <li>The Rust Programming Language</li>
        </ul>
      </div>
    </div>
  </div>
  <div class="footer">
    <p><a href="#">Subscribe</a> · <a href="#">Unsubscribe</a> · <a href="#">Archive</a></p>
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Yesterday", ts: 70,
    },
    // 6. Deployment notification
    Mail {
        account: 2, folder: 0, unread: false,
        from: "Vercel", from_email: "ship@vercel.com",
        subject: "Deployment ready",
        preview: "Your project was deployed successfully.",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #fff; color: #1a1a1a; }
  .container { max-width: 480px; margin: 0 auto; border: 1px solid #e5e5e5; }
  .status-bar { background: #50c878; height: 4px; }
  .header { padding: 30px; text-align: center; }
  .check { width: 48px; height: 48px; background: #50c878; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; color: #fff; font-size: 24px; }
  .header h2 { margin: 16px 0 4px; }
  .header p { color: #888; margin: 0; font-size: 14px; }
  .details { padding: 0 30px 30px; }
  .detail-row { display: flex; justify-content: space-between; padding: 12px 0; border-bottom: 1px solid #f5f5f5; font-size: 14px; }
  .detail-row .label { color: #888; }
  .detail-row .value { font-weight: 500; }
  .url { color: #0070f3; text-decoration: none; word-break: break-all; }
  .btn { display: block; text-align: center; background: #1a1a1a; color: #fff; padding: 14px; border-radius: 8px; text-decoration: none; font-weight: 500; margin-top: 20px; }
  .footer { padding: 20px 30px; text-align: center; font-size: 12px; color: #bbb; border-top: 1px solid #f0f0f0; }
</style>
</head>
<body>
<div class="container">
  <div class="status-bar"></div>
  <div class="header">
    <div class="check">✓</div>
    <h2>Production Deployed</h2>
    <p>photon-mail-app · 32s build time</p>
  </div>
  <div class="details">
    <div class="detail-row">
      <span class="label">URL</span>
      <span class="value"><a class="url" href="#">photon-mail.vercel.app</a></span>
    </div>
    <div class="detail-row">
      <span class="label">Commit</span>
      <span class="value">a3f8c2d</span>
    </div>
    <div class="detail-row">
      <span class="label">Branch</span>
      <span class="value">main</span>
    </div>
    <div class="detail-row">
      <span class="label">Author</span>
      <span class="value">@zytronium</span>
    </div>
    <a class="btn" href="#">View Analytics →</a>
  </div>
  <div class="footer">
    Powered by Vercel
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Yesterday", ts: 65,
    },
    // 7. HR benefits update
    Mail {
        account: 1, folder: 0, unread: false,
        from: "HR", from_email: "hr@acmecorp.com",
        subject: "Benefits update",
        preview: "Open enrollment starts next week...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #f8f9fa; color: #212529; }
  .container { max-width: 540px; margin: 0 auto; background: #fff; }
  .header { background: #0070f3; padding: 24px 30px; color: #fff; }
  .header h2 { margin: 0; }
  .content { padding: 30px; }
  .content p { line-height: 1.7; color: #495057; }
  .highlight-box { background: #e7f1ff; border: 1px solid #b3d4fc; border-radius: 8px; padding: 16px 20px; margin: 20px 0; }
  .highlight-box strong { color: #0070f3; }
  .timeline { list-style: none; padding: 0; margin: 20px 0; }
  .timeline li { padding: 10px 0 10px 24px; position: relative; border-left: 2px solid #0070f3; margin-left: 8px; }
  .timeline li::before { content: ""; position: absolute; left: -6px; top: 14px; width: 10px; height: 10px; background: #0070f3; border-radius: 50%; }
  .timeline .date { font-weight: bold; color: #0070f3; font-size: 13px; }
  .timeline .event { color: #495057; font-size: 14px; }
  .signature { margin-top: 24px; padding-top: 16px; border-top: 1px solid #eee; }
  .signature .name { font-weight: bold; }
  .signature .title { color: #868e96; font-size: 13px; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h2>📋 Benefits Open Enrollment</h2>
  </div>
  <div class="content">
    <p>Hi Team,</p>
    <p>Open enrollment for 2027 benefits starts next week. Here's everything you need to know:</p>

    <div class="highlight-box">
      <strong>⏰ Deadline:</strong> All changes must be submitted by <strong>December 15, 2026</strong> to take effect January 1, 2027.
    </div>

    <ul class="timeline">
      <li>
        <div class="date">Nov 24</div>
        <div class="event">Open enrollment begins</div>
      </li>
      <li>
        <div class="date">Nov 28</div>
        <div class="event">Benefits webinar (2 PM PT)</div>
      </li>
      <li>
        <div class="date">Dec 15</div>
        <div class="event">Enrollment deadline</div>
      </li>
      <li>
        <div class="date">Jan 1, 2027</div>
        <div class="event">New coverage begins</div>
      </li>
    </ul>

    <p>New this year: <strong>expanded mental health coverage</strong> and a <strong>401(k) match increase</strong> from 4% to 6%.</p>

    <div class="signature">
      <div class="name">Sarah Chen</div>
      <div class="title">People Operations · Acme Corp</div>
    </div>
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 12", ts: 55,
    },
    // 8. GitHub security advisory
    Mail {
        account: 2, folder: 0, unread: false,
        from: "GitHub Security", from_email: "security-noreply@github.com",
        subject: "⚠️ Security advisory: dependency vulnerability",
        preview: "A critical vulnerability was found in a dependency...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #0d1117; color: #c9d1d9; }
  .container { max-width: 560px; margin: 0 auto; }
  .alert { background: #2a1f00; border: 1px solid #6e4e00; border-radius: 6px; padding: 16px 20px; margin: 20px; }
  .alert h3 { margin: 0 0 8px; color: #f08c28; font-size: 16px; }
  .alert p { margin: 0; font-size: 14px; line-height: 1.6; }
  .content { padding: 0 20px 20px; }
  .content h2 { font-size: 18px; margin: 0 0 12px; color: #f0f6fc; }
  .content p { font-size: 14px; line-height: 1.7; color: #8b949e; }
  code { background: #161b22; padding: 2px 6px; border-radius: 4px; font-size: 13px; color: #79c0ff; }
  pre { background: #161b22; padding: 14px; border-radius: 6px; overflow-x: auto; font-size: 13px; line-height: 1.5; }
  .severity { display: inline-block; background: #da3633; color: #fff; padding: 2px 10px; border-radius: 12px; font-size: 12px; font-weight: bold; }
  .btn { display: inline-block; background: #238636; color: #fff; padding: 10px 20px; border-radius: 6px; text-decoration: none; font-size: 14px; font-weight: 500; margin-top: 12px; }
  .footer { padding: 20px; text-align: center; font-size: 12px; color: #484f58; border-top: 1px solid #21262d; }
</style>
</head>
<body>
<div class="container">
  <div class="alert">
    <h3>🔒 Security Advisory</h3>
    <p>A <span class="severity">CRITICAL</span> vulnerability has been identified in a dependency of your repository.</p>
  </div>
  <div class="content">
    <h2>Vulnerability in <code>reqwest</code> v0.11.23</h2>
    <p>A remote code execution vulnerability (CVE-2026-1234) was discovered in the <code>reqwest</code> crate. An attacker could exploit this via crafted HTTP headers.</p>
    <p><strong>Affected repositories:</strong></p>
    <pre>zytronium/photon-mail
zytronium/api-gateway</pre>
    <p><strong>Recommended fix:</strong> Update to <code>reqwest</code> v0.12.0 or later.</p>
    <a class="btn" href="#">View Security Alert →</a>
  </div>
  <div class="footer">
    You are receiving this because you are subscribed to this repository.
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 8", ts: 45,
    },
    // 9. Plain text reply
    Mail {
        account: 0, folder: 2, unread: false,
        from: "To: Alice", from_email: "foogle_user@femail.com",
        subject: "Re: Weekend plans",
        preview: "Saturday works for me!",
        body: "Saturday works for me! How about 2 PM at the usual spot?\n\nSee you then! :)",
        content_type: MailContentType::PlainText,
        date: "Nov 11", ts: 40,
    },
    // 10. Draft
    Mail {
        account: 1, folder: 1, unread: false,
        from: "Draft", from_email: "why.wont@it.work",
        subject: "Q4 report",
        preview: "Hi team, attached is the Q4 summary...",
        body: "Hi team,\n\nAttached is the Q4 summary report. Please review before our meeting on Friday.\n\nKey highlights:\n- Revenue up 15% YoY\n- New customer acquisition exceeded targets\n- Infrastructure costs reduced by 22%\n\nBest regards",
        content_type: MailContentType::PlainText,
        date: "Nov 10", ts: 35,
    },
    // 11. HTML event invitation
    Mail {
        account: 1, folder: 0, unread: false,
        from: "Events", from_email: "events@acmecorp.com",
        subject: "🎉 Company All-Hands — Dec 5",
        preview: "Join us for the quarterly all-hands meeting...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #1a1a2e; color: #e5e5f5; }
  .container { max-width: 500px; margin: 0 auto; background: #16213e; border-radius: 16px; overflow: hidden; }
  .hero { background: linear-gradient(135deg, #0f3460, #533483); padding: 40px 30px; text-align: center; }
  .hero .emoji { font-size: 48px; }
  .hero h1 { margin: 12px 0 0; font-size: 26px; color: #fff; }
  .content { padding: 30px; }
  .info-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin: 20px 0; }
  .info-card { background: #0f3460; border-radius: 10px; padding: 16px; text-align: center; }
  .info-card .icon { font-size: 24px; }
  .info-card .label { font-size: 11px; color: #9696aa; text-transform: uppercase; letter-spacing: 1px; margin-top: 4px; }
  .info-card .value { font-size: 15px; font-weight: bold; margin-top: 4px; }
  .agenda { margin: 20px 0; }
  .agenda-item { display: flex; gap: 12px; padding: 10px 0; border-bottom: 1px solid #0f3460; }
  .agenda-item .time { color: #6d4aff; font-weight: bold; font-size: 13px; min-width: 60px; }
  .agenda-item .topic { font-size: 14px; }
  .rsvp { display: block; text-align: center; background: #6d4aff; color: #fff; padding: 14px; border-radius: 10px; text-decoration: none; font-weight: bold; font-size: 16px; margin-top: 20px; }
</style>
</head>
<body>
<div class="container">
  <div class="hero">
    <div class="emoji">🎉</div>
    <h1>Quarterly All-Hands</h1>
  </div>
  <div class="content">
    <p>Join us for our Q4 all-hands meeting! We'll share updates, celebrate wins, and preview what's coming in 2027.</p>

    <div class="info-grid">
      <div class="info-card">
        <div class="icon">📅</div>
        <div class="label">Date</div>
        <div class="value">Dec 5, 2026</div>
      </div>
      <div class="info-card">
        <div class="icon">🕐</div>
        <div class="label">Time</div>
        <div class="value">3:00 PM PT</div>
      </div>
      <div class="info-card">
        <div class="icon">📍</div>
        <div class="label">Location</div>
        <div class="value">Main Auditorium</div>
      </div>
      <div class="info-card">
        <div class="icon">⏱️</div>
        <div class="label">Duration</div>
        <div class="value">90 min</div>
      </div>
    </div>

    <div class="agenda">
      <h3 style="margin: 0 0 12px;">Agenda</h3>
      <div class="agenda-item">
        <span class="time">3:00</span>
        <span class="topic">Welcome &amp; Q4 Recap</span>
      </div>
      <div class="agenda-item">
        <span class="time">3:20</span>
        <span class="topic">Product Updates</span>
      </div>
      <div class="agenda-item">
        <span class="time">3:40</span>
        <span class="topic">Engineering Deep Dive</span>
      </div>
      <div class="agenda-item">
        <span class="time">4:00</span>
        <span class="topic">2027 Roadmap Preview</span>
      </div>
      <div class="agenda-item">
        <span class="time">4:20</span>
        <span class="topic">Q&amp;A + Pizza 🍕</span>
      </div>
    </div>

    <a class="rsvp" href="#">RSVP Now</a>
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 9", ts: 30,
    },
    // 12. HTML password reset
    Mail {
        account: 0, folder: 0, unread: false,
        from: "Foogle Security", from_email: "no-reply@security.foogle.com",
        subject: "Password reset requested",
        preview: "We received a request to reset your password...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #f0f2f5; color: #1a1a2e; }
  .container { max-width: 460px; margin: 0 auto; background: #fff; border-radius: 12px; overflow: hidden; box-shadow: 0 2px 8px rgba(0,0,0,0.08); }
  .header { background: #6d4aff; padding: 24px 30px; text-align: center; }
  .header h2 { margin: 0; color: #fff; font-size: 20px; }
  .content { padding: 30px; text-align: center; }
  .content p { color: #555; line-height: 1.7; font-size: 15px; }
  .code { display: inline-block; background: #f8f8ff; border: 2px dashed #6d4aff; padding: 16px 28px; border-radius: 10px; font-size: 28px; font-weight: bold; letter-spacing: 8px; color: #6d4aff; margin: 20px 0; font-family: "SF Mono", monospace; }
  .btn { display: inline-block; background: #6d4aff; color: #fff; padding: 14px 36px; border-radius: 8px; text-decoration: none; font-weight: bold; font-size: 15px; margin: 16px 0; }
  .warning { background: #fff8e1; border: 1px solid #ffecb3; border-radius: 8px; padding: 12px 16px; margin: 20px 0; font-size: 13px; color: #856404; text-align: left; }
  .footer { padding: 20px 30px; text-align: center; font-size: 12px; color: #aaa; border-top: 1px solid #f0f0f0; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h2>🔐 Foogle Security</h2>
  </div>
  <div class="content">
    <p>We received a request to reset the password for your account <strong>foogle_user@femail.com</strong>.</p>
    <p>Use the verification code below:</p>
    <div class="code">847291</div>
    <p style="font-size: 13px; color: #888;">This code expires in 10 minutes.</p>
    <a class="btn" href="#">Reset Password</a>
    <div class="warning">
      ⚠️ If you didn't request a password reset, please secure your account immediately by contacting support.
    </div>
  </div>
  <div class="footer">
    © 2026 Foogle Inc. · This email was sent to foogle_user@femail.com
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 7", ts: 25,
    },
    // 13. Plain text archived message
    Mail {
        account: 2, folder: 3, unread: false,
        from: "GitHub", from_email: "security-noreply@github.com",
        subject: "Security advisory (archived)",
        preview: "A dependency in your repo had a vulnerability...",
        body: "A dependency in your repo had a vulnerability that has since been resolved.\n\nThis message has been archived.",
        content_type: MailContentType::PlainText,
        date: "Nov 8", ts: 20,
    },
    // 14. HTML table / data report
    Mail {
        account: 1, folder: 0, unread: false,
        from: "Analytics", from_email: "reports@analytics.io",
        subject: "📊 Weekly Traffic Report",
        preview: "Your weekly analytics summary is ready...",
        body: r##"<!DOCTYPE html>
<html>
<head>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #fafafa; color: #333; }
  .container { max-width: 560px; margin: 0 auto; background: #fff; }
  .header { padding: 24px 30px; border-bottom: 3px solid #6d4aff; }
  .header h2 { margin: 0; }
  .header .period { color: #888; font-size: 13px; margin-top: 4px; }
  .content { padding: 24px 30px; }
  .kpi-row { display: flex; gap: 12px; margin-bottom: 24px; }
  .kpi { flex: 1; background: #f8f8ff; border-radius: 10px; padding: 16px; text-align: center; }
  .kpi .value { font-size: 24px; font-weight: bold; color: #6d4aff; }
  .kpi .label { font-size: 12px; color: #888; margin-top: 4px; }
  .kpi .change { font-size: 12px; margin-top: 4px; }
  .kpi .change.up { color: #50c878; }
  .kpi .change.down { color: #dc465a; }
  table { width: 100%; border-collapse: collapse; margin-top: 16px; }
  th { text-align: left; padding: 10px 12px; background: #f8f8ff; font-size: 12px; text-transform: uppercase; letter-spacing: 0.5px; color: #888; }
  td { padding: 10px 12px; border-bottom: 1px solid #f0f0f0; font-size: 14px; }
  .bar { height: 6px; border-radius: 3px; background: #6d4aff; }
  .footer { padding: 20px 30px; text-align: center; font-size: 12px; color: #bbb; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h2>📊 Weekly Traffic Report</h2>
    <div class="period">Nov 11 – Nov 17, 2026</div>
  </div>
  <div class="content">
    <div class="kpi-row">
      <div class="kpi">
        <div class="value">24.8K</div>
        <div class="label">Page Views</div>
        <div class="change up">↑ 12.3%</div>
      </div>
      <div class="kpi">
        <div class="value">3,421</div>
        <div class="label">Unique Visitors</div>
        <div class="change up">↑ 8.7%</div>
      </div>
      <div class="kpi">
        <div class="value">4:32</div>
        <div class="label">Avg. Duration</div>
        <div class="change down">↓ 2.1%</div>
      </div>
    </div>

    <h3 style="margin: 0 0 12px; font-size: 16px;">Top Pages</h3>
    <table>
      <thead>
        <tr><th>Page</th><th>Views</th><th>Trend</th></tr>
      </thead>
      <tbody>
        <tr>
          <td>/dashboard</td>
          <td>8,241</td>
          <td><div class="bar" style="width: 82%;"></div></td>
        </tr>
        <tr>
          <td>/settings</td>
          <td>4,102</td>
          <td><div class="bar" style="width: 41%;"></div></td>
        </tr>
        <tr>
          <td>/compose</td>
          <td>3,890</td>
          <td><div class="bar" style="width: 39%;"></div></td>
        </tr>
        <tr>
          <td>/inbox</td>
          <td>3,520</td>
          <td><div class="bar" style="width: 35%;"></div></td>
        </tr>
      </tbody>
    </table>
  </div>
  <div class="footer">
    Generated by Analytics · <a href="#" style="color: #6d4aff;">Full Report</a>
  </div>
</div>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 6", ts: 15,
    },
    // 15. Stellicast — email confirmation (dark theme)
    Mail {
        account: 0, folder: 0, unread: true,
        from: "Stellicast", from_email: "noreply@stellicast.com",
        subject: "Welcome to Stellicast",
        preview: "Confirm your email address to activate your account...",
        body: r##"<html>
<body style="margin: 0; padding: 0; font-family: 'Helvetica Neue', Helvetica, Arial, sans-serif; background: linear-gradient(to bottom, #020617, #000000); color: #ededed;">
<table width="100%" cellpadding="0" cellspacing="0" border="0" style="background: linear-gradient(to bottom, #020617, #000000); padding: 40px 20px;">
  <tr>
    <td align="center">
      <table width="600" cellpadding="0" cellspacing="0" border="0" style="max-width: 600px; background: linear-gradient(to bottom, rgba(30, 58, 138, 0.8), rgba(15, 23, 42, 0.9)); border-radius: 12px; overflow: hidden; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.5);">
        <!-- Header with Logo -->
        <tr>
          <td style="padding: 40px 40px 30px; text-align: center; background: rgba(0, 0, 0, 0.3);">
            <img src="https://stellicast.com/stellicast.png" alt="Stellicast" style="max-width: 250px; height: auto; display: block; margin: 0 auto;">
          </td>
        </tr>

        <!-- Main Content -->
        <tr>
          <td style="padding: 40px 40px 20px;">
            <h1 style="margin: 0 0 20px; font-size: 28px; font-weight: 600; color: #ffffff; text-align: center; letter-spacing: -0.5px;">
              Welcome to Stellicast
            </h1>
            <p style="margin: 0 0 24px; font-size: 16px; line-height: 1.6; color: #d1d5db; text-align: center;">
              You're one step away from joining the video platform built for creators and viewers.
            </p>
            <p style="margin: 0 0 32px; font-size: 16px; line-height: 1.6; color: #d1d5db; text-align: center;">
              Click the button below to confirm your email address and activate your account
            </p>

            <!-- CTA Button -->
            <table width="100%" cellpadding="0" cellspacing="0" border="0">
              <tr>
                <td align="center" style="padding: 0 0 32px;">
                  <a href="https://stellicast.com/confirm?token=abc123def456" style="display: inline-block; padding: 16px 48px; background: #2563eb; color: #ffffff; text-decoration: none; border-radius: 8px; font-size: 16px; font-weight: 600; box-shadow: 0 4px 14px rgba(37, 99, 235, 0.4); transition: background 0.2s;">
                    Confirm Your Email
                  </a>
                </td>
              </tr>
            </table>

            <p style="margin: 0 0 16px; font-size: 14px; line-height: 1.6; color: #9ca3af; text-align: center;">
              Or copy and paste this link into your browser:
            </p>
            <p style="margin: 0 0 32px; font-size: 13px; line-height: 1.6; color: #6b7280; text-align: center; word-break: break-all; padding: 12px; background: rgba(0, 0, 0, 0.3); border-radius: 6px;">
              https://stellicast.com/confirm?token=abc123def456
            </p>
          </td>
        </tr>

        <!-- Footer -->
        <tr>
          <td style="padding: 24px 40px 40px; border-top: 1px solid rgba(255, 255, 255, 0.1);">
            <p style="margin: 0 0 12px; font-size: 13px; line-height: 1.6; color: #6b7280; text-align: center;">
              This confirmation link will expire in 24 hours.
            </p>
            <p style="margin: 0; font-size: 13px; line-height: 1.6; color: #6b7280; text-align: center;">
              If you didn't create a Stellicast account, you can safely ignore this email.
            </p>
          </td>
        </tr>

        <!-- Branding Footer -->
        <tr>
          <td style="padding: 24px 40px; background: rgba(0, 0, 0, 0.4); text-align: center;">
            <p style="margin: 0 0 8px; font-size: 12px; color: #6b7280;">
              <strong style="color: #9ca3af;">Stellicast</strong>: Watch Without Being Watched
            </p>
            <p style="margin: 0; font-size: 11px; color: #4b5563;">
              Please note that Stellicast is in early development and your account may not always function correctly.
            </p>
          </td>
        </tr>
      </table>
    </td>
  </tr>
</table>
</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 5", ts: 10,
    },
    // 16. Luminous — email confirmation (dark navy theme with teal accents)
    Mail {
        account: 2, folder: 0, unread: true,
        from: "Luminous", from_email: "noreply@luminous-chat.app",
        subject: "Confirm your Luminous account",
        preview: "Welcome to Luminous! Verify your Atlas School email...",
        body: r##"<html>
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Confirm your Luminous account</title>
</head>
<body style="margin:0;padding:0;background-color:#00002e;font-family:Arial,Helvetica,sans-serif;">

<!-- Outer wrapper -->
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"
       style="background-color:#00002e;min-height:100vh;">
  <tr>
    <td align="center" valign="top" style="padding:40px 16px;">

      <!-- Card -->
      <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0"
             style="max-width:480px;background-color:#00003c;border-radius:32px;border:3px solid #fef9e6;overflow:hidden;">

        <!-- Teal top accent bar -->
        <tr>
          <td style="background-color:#1ED2AF;height:6px;font-size:0;line-height:0;">&nbsp;</td>
        </tr>

        <!-- Header -->
        <tr>
          <td align="center" style="padding:36px 40px 24px;">
            <img src="https://luminous-chat.vercel.app/logo.png"
                 alt="Luminous"
                 width="56" height="56"
                 style="border-radius:50%;display:block;margin:0 auto 16px;" />
            <h1 style="margin:0;font-size:28px;font-weight:700;color:#fef9e6;letter-spacing:-0.5px;">
              Luminous
            </h1>
            <p style="margin:6px 0 0;font-size:12px;color:#1ED2AF;letter-spacing:2px;text-transform:uppercase;">
              Atlas School's Unofficial Chat App
            </p>
          </td>
        </tr>

        <!-- Divider -->
        <tr>
          <td style="padding:0 40px;">
            <div style="height:1px;background-color:#fef9e6;opacity:0.1;"></div>
          </td>
        </tr>

        <!-- Body -->
        <tr>
          <td style="padding:32px 40px 16px;">
            <h2 style="margin:0 0 12px;font-size:20px;font-weight:700;color:#fef9e6;">
              Confirm your email address
            </h2>
            <p style="margin:0 0 20px;font-size:15px;line-height:1.6;color:#fef9e6;opacity:0.75;">
              Welcome to Luminous! To finish creating your account, please verify
              that this is your Atlas School email address by clicking the button below.
            </p>
            <p style="margin:0 0 28px;font-size:15px;line-height:1.6;color:#fef9e6;opacity:0.75;">
              This link will expire in <strong style="color:#fef9e6;opacity:1;">24 hours</strong>.
            </p>
          </td>
        </tr>

        <!-- CTA Button -->
        <tr>
          <td align="center" style="padding:0 40px 32px;">
            <table role="presentation" cellpadding="0" cellspacing="0" border="0">
              <tr>
                <td align="center" style="border-radius:20px;background-color:#1ED2AF;">
                  <a href="https://luminous-chat.vercel.app/confirm?token=xyz789ghi012"
                     style="display:inline-block;padding:14px 36px;font-size:15px;font-weight:700;color:#00002e;text-decoration:none;border-radius:20px;letter-spacing:0.3px;">
                    Confirm My Account
                  </a>
                </td>
              </tr>
            </table>
          </td>
        </tr>

        <!-- Fallback link -->
        <tr>
          <td style="padding:0 40px 32px;">
            <div style="background-color:#00002e;border-radius:16px;padding:16px;border:1px solid #fef9e6;border-opacity:0.1;">
              <p style="margin:0 0 8px;font-size:11px;color:#fef9e6;opacity:0.4;text-transform:uppercase;letter-spacing:1.5px;">
                Button not working?
              </p>
              <p style="margin:0;font-size:12px;color:#fef9e6;opacity:0.5;word-break:break-all;line-height:1.5;">
                Copy and paste this link into your browser:<br/>
                <a href="https://luminous-chat.vercel.app/confirm?token=xyz789ghi012"
                   style="color:#1ED2AF;text-decoration:underline;font-size:12px;word-break:break-all;">
                  https://luminous-chat.vercel.app/confirm?token=xyz789ghi012
                </a>
              </p>
            </div>
          </td>
        </tr>

        <!-- Divider -->
        <tr>
          <td style="padding:0 40px;">
            <div style="height:1px;background-color:#fef9e6;opacity:0.1;"></div>
          </td>
        </tr>

        <!-- Footer -->
        <tr>
          <td align="center" style="padding:24px 40px 32px;">
            <p style="margin:0 0 6px;font-size:12px;color:#fef9e6;opacity:0.35;line-height:1.5;">
              If you didn't create a Luminous account, you can safely ignore this email.
            </p>
          </td>
        </tr>

        <!-- Teal bottom accent bar -->
        <tr>
          <td style="background-color:#1ED2AF;height:4px;font-size:0;line-height:0;">&nbsp;</td>
        </tr>

      </table>
      <!-- /Card -->

    </td>
  </tr>
</table>

</body>
</html>"##,
        content_type: MailContentType::Html,
        date: "Nov 4", ts: 5,
    },
];
