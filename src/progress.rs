//! The readiness bar in the READMEs, counted from the marks in ROADMAP.md:
//! ✅ counts as done, 🟡 as half, ⬜ as not started.

use std::path::Path;

const BAR: usize = 20;

struct Counts {
    done: usize,
    partial: usize,
    todo: usize,
}

impl Counts {
    fn percent(&self) -> usize {
        let total = self.done + self.partial + self.todo;
        ((self.done as f64 + self.partial as f64 / 2.0) / total as f64 * 100.0).round() as usize
    }

    fn bar(&self) -> String {
        let filled = (self.percent() * BAR + 50) / 100;
        format!("{}{}", "█".repeat(filled), "░".repeat(BAR - filled))
    }
}

/// Items of the roadmap itself; the time estimate at the end has no marks to count.
fn count(roadmap: &str) -> Counts {
    let items = roadmap.split("## Time estimate").next().unwrap_or_default();
    let mark = |m: &str| items.lines().filter(|l| l.trim_start().starts_with(&format!("- {m}"))).count();
    Counts { done: mark("✅"), partial: mark("🟡"), todo: mark("⬜") }
}

fn block(c: &Counts, uk: bool) -> String {
    let (p, bar) = (c.percent(), c.bar());
    let line = if uk {
        format!("**Готовність за дорожньою картою: {p}%** `{bar}` — {} готово, {} частково, {} попереду ([ROADMAP.md](ROADMAP.md))", c.done, c.partial, c.todo)
    } else {
        format!("**Roadmap progress: {p}%** `{bar}` — {} done, {} partial, {} to go ([ROADMAP.md](ROADMAP.md))", c.done, c.partial, c.todo)
    };
    format!("<!-- progress -->\n{line}\n<!-- /progress -->")
}

/// The bar in both READMEs must match ROADMAP.md. After changing marks:
/// `UPDATE_PROGRESS=1 cargo test progress` rewrites it.
#[test]
fn readme_progress_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let counts = count(&std::fs::read_to_string(root.join("ROADMAP.md")).unwrap());
    assert!(counts.done + counts.partial + counts.todo > 100, "the roadmap scanner found too few items");
    for (file, uk) in [("README.md", false), ("README.uk.md", true)] {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        let (start, end) = (text.find("<!-- progress -->").unwrap(), text.find("<!-- /progress -->").unwrap() + "<!-- /progress -->".len());
        let want = format!("{}{}{}", &text[..start], block(&counts, uk), &text[end..]);
        if std::env::var_os("UPDATE_PROGRESS").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        assert!(std::fs::read_to_string(&path).unwrap() == want, "{file}: progress is out of date: run `UPDATE_PROGRESS=1 cargo test progress`");
    }
}
