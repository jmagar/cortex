//! Small suggestion helpers for the hand-rolled CLI parser.

pub(crate) fn did_you_mean<'a>(input: &str, candidates: &'a [&'a str]) -> Option<&'a str> {
    // Match on the flag/command name only — drop any `=value` so equals-style
    // options (`--projct=foo`) still suggest the right flag (`--project`).
    let name = input.split('=').next().unwrap_or(input);
    let normalized = name.trim_start_matches('-');
    let mut best: Option<(&str, usize)> = None;
    for &candidate in candidates {
        let candidate_cmp = candidate.trim_start_matches('-');
        let distance = levenshtein(normalized, candidate_cmp);
        let threshold = if candidate_cmp.len() <= 4 { 1 } else { 3 };
        if distance <= threshold && best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(candidate, _)| candidate)
}

pub(crate) fn unknown_command(kind: &str, input: &str, candidates: &[&str]) -> String {
    match did_you_mean(input, candidates) {
        Some(candidate) => {
            format!("unknown {kind}: {input}\n\nDid you mean `{candidate}`?")
        }
        None => format!("unknown {kind}: {input}"),
    }
}

pub(crate) fn unknown_option(command: &str, input: &str, candidates: &[&str]) -> String {
    match did_you_mean(input, candidates) {
        Some(candidate) => {
            format!("unknown {command} option: {input}\n\nDid you mean `{candidate}`?")
        }
        None => format!("unknown {command} option: {input}"),
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, ac) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b_chars.iter().enumerate() {
            let substitution = prev[j] + usize::from(ac != *bc);
            let insertion = curr[j] + 1;
            let deletion = prev[j + 1] + 1;
            curr[j + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;
