//! nd-replay: run recorded (or synthetic) sensing traces through the engine.
//!
//!   nd-replay run <trace.jsonl> [--labels labels.csv]   timeline (+ scores)
//!   nd-replay check <dir>                               every trace with labels must meet the targets
//!   nd-replay synth <dir>                               write synthetic scenario traces + labels

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nd_core::eval::{evaluate, parse_labels, EvalResult, Targets};
use nd_core::sim::{self, Scenario};
use nd_core::{Calibration, Config, SignalFrame};

fn load_trace(path: &Path) -> Result<(Vec<SignalFrame>, Option<Calibration>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut frames = Vec::new();
    let mut cal = None;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // The recorder writes the calibration as the first line.
        if i == 0 && line.contains("\"screen_pitch\"") {
            cal = Some(serde_json::from_str(line).map_err(|e| format!("{}:1: {e}", path.display()))?);
            continue;
        }
        frames.push(serde_json::from_str(line).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))?);
    }
    Ok((frames, cal))
}

fn labels_for(trace: &Path) -> PathBuf {
    trace.with_extension("labels.csv")
}

fn mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn print_scores(r: &EvalResult) {
    println!(
        "  agreement {:.1}%  confident {:.1}%  focus error {:.1}%  distraction→focus {:.1}%  unclear {:.1}%  nudges {} (false {})",
        r.agreement() * 100.0,
        r.confident_agreement() * 100.0,
        r.focus_error() * 100.0,
        r.distraction_as_focus() * 100.0,
        r.unclear_share() * 100.0,
        r.nudges,
        r.false_nudges
    );
}

fn print_confusion(r: &EvalResult) {
    println!("  confusion (truth → predicted: seconds)");
    for ((t, p), n) in &r.confusion {
        let mark = if t == p { " " } else { "!" };
        println!("   {mark} {t:>11} → {p:<11} {n}");
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let trace = PathBuf::from(args.first().ok_or("usage: nd-replay run <trace.jsonl> [--labels file]")?);
    let (frames, cal) = load_trace(&trace)?;
    let cal = cal.unwrap_or_default();
    let labels_path = args.iter().position(|a| a == "--labels").and_then(|i| args.get(i + 1)).map(PathBuf::from).or_else(|| {
        let p = labels_for(&trace);
        p.exists().then_some(p)
    });
    let labels = match &labels_path {
        Some(p) => parse_labels(&std::fs::read_to_string(p).map_err(|e| e.to_string())?)?,
        None => Vec::new(),
    };
    let t0 = frames.first().map_or(0, |f| f.t_ms);
    let r = evaluate(&frames, &labels, &Config::default(), &cal);
    println!("{} — {} frames", trace.display(), frames.len());
    for (a, b, act, detail) in &r.timeline {
        println!("  {} – {}  {:<12} {}", mmss(a - t0), mmss(b - t0), act.as_str(), detail.as_deref().unwrap_or(""));
    }
    if !labels.is_empty() {
        print_scores(&r);
        print_confusion(&r);
    }
    Ok(true)
}

fn check(args: &[String]) -> Result<bool, String> {
    let dir = PathBuf::from(args.first().ok_or("usage: nd-replay check <dir>")?);
    let mut traces: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && labels_for(p).exists())
        .collect();
    traces.sort();
    if traces.is_empty() {
        return Err(format!("no labeled traces in {}", dir.display()));
    }
    let targets = Targets::default();
    let mut passed = 0;
    for t in &traces {
        let (frames, cal) = load_trace(t)?;
        let labels = parse_labels(&std::fs::read_to_string(labels_for(t)).map_err(|e| e.to_string())?)?;
        let r = evaluate(&frames, &labels, &Config::default(), &cal.unwrap_or_default());
        let fails = targets.failures(&r);
        println!("{} {}", if fails.is_empty() { "pass" } else { "FAIL" }, t.file_name().unwrap_or_default().to_string_lossy());
        print_scores(&r);
        for f in &fails {
            println!("  ✗ {f}");
        }
        if fails.is_empty() {
            passed += 1;
        } else {
            print_confusion(&r);
        }
    }
    println!("{passed} of {} traces meet the targets", traces.len());
    Ok(passed == traces.len())
}

fn synth(args: &[String]) -> Result<bool, String> {
    let dir = PathBuf::from(args.first().ok_or("usage: nd-replay synth <dir>")?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cal = Calibration { calibrated: true, ..Default::default() };
    let mut scripts: Vec<(String, Vec<(Scenario, u32)>)> =
        Scenario::ALL.iter().map(|s| (s.name().to_string(), vec![(Scenario::Typing, 60), (*s, 300)])).collect();
    scripts.push((
        "mixed-session".into(),
        vec![
            (Scenario::Typing, 300),
            (Scenario::Reading, 420),
            (Scenario::Phone, 120),
            (Scenario::Writing, 300),
            (Scenario::Instagram, 90),
            (Scenario::LeftDesk, 200),
            (Scenario::StaringAway, 120),
            (Scenario::Typing, 180),
        ],
    ));
    for (name, script) in scripts {
        let (frames, labels) = sim::session(&script, &cal, 7);
        let mut out = serde_json::to_string(&cal).map_err(|e| e.to_string())? + "\n";
        for f in &frames {
            out.push_str(&serde_json::to_string(f).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        let trace = dir.join(format!("synthetic-{name}.jsonl"));
        std::fs::write(&trace, out).map_err(|e| e.to_string())?;
        let mut csv = String::from("start_ms,end_ms,label\n");
        for (a, b, act) in labels {
            csv.push_str(&format!("{a},{b},{}\n", act.as_str()));
        }
        std::fs::write(labels_for(&trace), csv).map_err(|e| e.to_string())?;
        println!("wrote {}", trace.display());
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("synth") => synth(&args[1..]),
        _ => Err("usage: nd-replay <run|check|synth> ...".into()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
