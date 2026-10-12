//! Which apps and sites count as focus, neutral, or drift.

use serde::{Deserialize, Serialize};

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Focus,
    Neutral,
    Drift,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleKind {
    App,
    Domain,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// A domain ("youtube.com", matches subdomains), a bundle id, or an app
    /// name. A trailing `*` matches any suffix ("com.jetbrains.*").
    pub pattern: String,
    pub kind: RuleKind,
    pub category: Category,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Categorized {
    pub category: Category,
    /// Human label for drift causes: a host or an app name.
    pub detail: Option<String>,
    /// Browser host, when one is known.
    pub host: Option<String>,
    /// Whether a rule matched (as opposed to the Neutral fallback).
    pub matched: bool,
}

const BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "org.mozilla.firefox",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "org.chromium.Chromium",
    "app.zen-browser.zen",
];

pub fn is_browser(bundle: &str) -> bool {
    BROWSERS.contains(&bundle)
}

/// Window-title fallback for when the exact URL isn't available.
const TITLE_HINTS: &[(&str, &str)] = &[
    ("YouTube", "youtube.com"),
    ("Instagram", "instagram.com"),
    ("TikTok", "tiktok.com"),
    ("Reddit", "reddit.com"),
    ("Facebook", "facebook.com"),
    ("Twitch", "twitch.tv"),
    ("Netflix", "netflix.com"),
    ("Pinterest", "pinterest.com"),
    ("Threads", "threads.net"),
    (" / X", "x.com"),
    ("on X:", "x.com"),
];

pub fn host_from_title(title: &str) -> Option<String> {
    TITLE_HINTS
        .iter()
        .find(|(hint, _)| title.contains(hint))
        .map(|(_, host)| host.to_string())
}

pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    h.strip_prefix("www.").map(str::to_string).unwrap_or(h)
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    host == pattern || (host.len() > pattern.len() && host.ends_with(pattern) && host.as_bytes()[host.len() - pattern.len() - 1] == b'.')
}

fn app_matches(value: &str, pattern: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => value.eq_ignore_ascii_case(pattern),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rules {
    list: Vec<Rule>,
}

impl Rules {
    pub fn new(list: Vec<Rule>) -> Self {
        Self { list }
    }

    pub fn list(&self) -> &[Rule] {
        &self.list
    }

    pub fn set(&mut self, rule: Rule) {
        let rule = Rule {
            pattern: match rule.kind {
                RuleKind::Domain => normalize_host(&rule.pattern),
                RuleKind::App => rule.pattern.trim().to_string(),
            },
            ..rule
        };
        match self.list.iter_mut().find(|r| r.kind == rule.kind && r.pattern == rule.pattern) {
            Some(existing) => existing.category = rule.category,
            None => self.list.push(rule),
        }
    }

    pub fn remove(&mut self, pattern: &str, kind: RuleKind) {
        self.list.retain(|r| !(r.kind == kind && r.pattern == pattern));
    }

    /// Longest matching domain rule wins, so "docs.google.com" can be focus
    /// while "google.com" stays neutral.
    fn match_domain(&self, host: &str) -> Option<&Rule> {
        self.list
            .iter()
            .filter(|r| r.kind == RuleKind::Domain && domain_matches(host, &r.pattern))
            .max_by_key(|r| r.pattern.len())
    }

    fn match_app(&self, bundle: Option<&str>, name: Option<&str>) -> Option<&Rule> {
        self.list.iter().filter(|r| r.kind == RuleKind::App).find(|r| {
            bundle.is_some_and(|b| app_matches(b, &r.pattern)) || name.is_some_and(|n| app_matches(n, &r.pattern))
        })
    }

    pub fn categorize(
        &self,
        bundle: Option<&str>,
        app_name: Option<&str>,
        host: Option<&str>,
        title: Option<&str>,
    ) -> Categorized {
        let browser = bundle.is_some_and(is_browser);
        let host = host
            .filter(|h| !h.trim().is_empty())
            .map(normalize_host)
            .or_else(|| if browser { title.and_then(host_from_title) } else { None });

        if let Some(h) = &host {
            if let Some(rule) = self.match_domain(h) {
                return Categorized { category: rule.category, detail: Some(h.clone()), host: Some(h.clone()), matched: true };
            }
        }
        if !browser {
            if let Some(rule) = self.match_app(bundle, app_name) {
                return Categorized {
                    category: rule.category,
                    detail: app_name.or(bundle).map(str::to_string),
                    host: None,
                    matched: true,
                };
            }
        }
        Categorized {
            category: Category::Neutral,
            detail: host.clone().or_else(|| app_name.or(bundle).map(str::to_string)),
            host,
            matched: false,
        }
    }

    pub fn defaults() -> Vec<Rule> {
        let d = |p: &str, c| Rule { pattern: p.to_string(), kind: RuleKind::Domain, category: c };
        let a = |p: &str, c| Rule { pattern: p.to_string(), kind: RuleKind::App, category: c };
        use Category::*;
        vec![
            d("instagram.com", Drift),
            d("tiktok.com", Drift),
            d("x.com", Drift),
            d("twitter.com", Drift),
            d("facebook.com", Drift),
            d("reddit.com", Drift),
            d("threads.net", Drift),
            d("youtube.com", Drift),
            d("twitch.tv", Drift),
            d("netflix.com", Drift),
            d("pinterest.com", Drift),
            d("9gag.com", Drift),
            d("tumblr.com", Drift),
            d("discord.com", Drift),
            d("docs.google.com", Focus),
            d("notion.so", Focus),
            d("overleaf.com", Focus),
            d("khanacademy.org", Focus),
            d("coursera.org", Focus),
            d("edx.org", Focus),
            d("instructure.com", Focus),
            d("scholar.google.com", Focus),
            d("arxiv.org", Focus),
            d("quizlet.com", Focus),
            d("wolframalpha.com", Focus),
            d("desmos.com", Focus),
            a("com.hnc.Discord", Drift),
            a("com.valvesoftware.steam", Drift),
            a("com.apple.dt.Xcode", Focus),
            a("com.microsoft.VSCode", Focus),
            a("com.todesktop.230313mzl4w4u92", Focus),
            a("com.jetbrains.*", Focus),
            a("notion.id", Focus),
            a("md.obsidian", Focus),
            a("com.goodnotesapp.x", Focus),
            a("net.ankiweb.dtop", Focus),
            a("org.zotero.zotero", Focus),
            a("com.apple.iWork.Pages", Focus),
            a("com.apple.iWork.Numbers", Focus),
            a("com.apple.iWork.Keynote", Focus),
            a("com.microsoft.Word", Focus),
            a("com.microsoft.Excel", Focus),
            a("com.microsoft.Powerpoint", Focus),
            a("com.readdle.PDFExpert-Mac", Focus),
            a("com.apple.Preview", Focus),
            a("com.apple.Notes", Focus),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::new(Rules::defaults())
    }

    #[test]
    fn domains_match_subdomains_but_not_lookalikes() {
        let r = rules();
        let c = r.categorize(Some("com.apple.Safari"), Some("Safari"), Some("m.youtube.com"), None);
        assert_eq!(c.category, Category::Drift);
        let c = r.categorize(Some("com.apple.Safari"), Some("Safari"), Some("notyoutube.com"), None);
        assert_eq!(c.category, Category::Neutral);
        assert!(!c.matched);
    }

    #[test]
    fn longest_domain_wins() {
        let mut r = rules();
        r.set(Rule { pattern: "google.com".into(), kind: RuleKind::Domain, category: Category::Drift });
        let c = r.categorize(Some("com.google.Chrome"), None, Some("docs.google.com"), None);
        assert_eq!(c.category, Category::Focus);
    }

    #[test]
    fn title_fallback_for_browsers_only() {
        let r = rules();
        let c = r.categorize(Some("org.mozilla.firefox"), Some("Firefox"), None, Some("cats - YouTube — Mozilla Firefox"));
        assert_eq!(c.category, Category::Drift);
        assert_eq!(c.detail.as_deref(), Some("youtube.com"));
        let c = r.categorize(Some("com.apple.TextEdit"), Some("TextEdit"), None, Some("YouTube script.txt"));
        assert_eq!(c.category, Category::Neutral);
    }

    #[test]
    fn app_rules_and_wildcards() {
        let r = rules();
        assert_eq!(r.categorize(Some("com.jetbrains.pycharm"), None, None, None).category, Category::Focus);
        assert_eq!(r.categorize(Some("com.hnc.Discord"), Some("Discord"), None, None).category, Category::Drift);
    }

    #[test]
    fn set_replaces_existing_rule() {
        let mut r = rules();
        r.set(Rule { pattern: "www.YouTube.com".into(), kind: RuleKind::Domain, category: Category::Focus });
        let c = r.categorize(Some("com.apple.Safari"), None, Some("youtube.com"), None);
        assert_eq!(c.category, Category::Focus);
        assert_eq!(r.list().iter().filter(|x| x.pattern == "youtube.com").count(), 1);
    }
}
