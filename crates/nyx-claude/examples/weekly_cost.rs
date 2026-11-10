//! Read-only calibration helper: estimated cost of the last 7 days across all local
//! transcripts. Prints only aggregates. `cargo run -p nyx-claude --example weekly_cost`
use std::collections::HashSet;

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map_or(false, |x| x == "jsonl") {
                out.push(p);
            }
        }
    }
}

fn main() {
    let mut files = Vec::new();
    walk(&nyx_claude::projects_dir(), &mut files);
    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
    let mut seen = HashSet::new();
    let (mut cost, mut msgs) = (0.0f64, 0u64);
    let mut by_day: std::collections::BTreeMap<String, f64> = Default::default();
    for f in &files {
        if let Ok((recs, _)) = nyx_claude::read_from(f, 0) {
            for r in recs {
                if r.ts >= cutoff && seen.insert(r.message_id.clone()) {
                    let c = nyx_claude::estimate_cost_usd(&r);
                    cost += c;
                    msgs += 1;
                    *by_day.entry(r.ts.format("%Y-%m-%d").to_string()).or_default() += c;
                }
            }
        }
    }
    println!("files={} messages={} cost_7d_usd={:.2}", files.len(), msgs, cost);
    for (d, c) in by_day {
        println!("{d} {c:.2}");
    }
}
