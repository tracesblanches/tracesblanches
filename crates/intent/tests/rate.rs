//! Rate of good classification on the question sets (A11, #98), printed with the
//! confusion matrix (`cargo test -p skimap-intent --test rate -- --nocapture`).

use skimap_intent::measure::{evaluate, parse};

fn run(name: &str, tsv: &str) -> skimap_intent::measure::Report {
    let cases = parse(tsv).expect("set parses");
    let r = evaluate(&cases);
    println!("== {name} ({} questions)\n{r}", cases.len());
    r
}

/// The held-out set: never used to tune the lexicon or the weights.
#[test]
fn held_out_set() {
    let r = run("test.tsv (held out)", include_str!("data/test.tsv"));
    assert!(r.total >= 100, "at least 100 questions");
    assert!(r.accuracy() >= 0.90, "accuracy {:.3}", r.accuracy());
    // Doing something else than asked is worse than asking again.
    assert!(r.wrong_command <= 2, "wrong commands: {}", r.wrong_command);
}

/// The development set: what the lexicon and the weights were tuned on.
#[test]
fn development_set() {
    let r = run("dev.tsv", include_str!("data/dev.tsv"));
    assert!(r.total >= 100);
    assert!(r.accuracy() >= 0.95, "accuracy {:.3}", r.accuracy());
}
