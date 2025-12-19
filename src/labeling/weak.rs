//! Weak labeling rules derived from filenames and folder names.

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

pub const WEAK_LABEL_RULESET_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct WeakLabel {
    pub class_id: &'static str,
    pub confidence: f32,
    pub rule_id: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum MatchTarget {
    FileStem,
    FullPath,
}

#[derive(Debug)]
struct Rule {
    id: &'static str,
    class_id: &'static str,
    confidence: f32,
    target: MatchTarget,
    regex: Regex,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut rules = Vec::new();

        let mut push = |id: &'static str,
                        class_id: &'static str,
                        confidence: f32,
                        target: MatchTarget,
                        pattern: &'static str| {
            rules.push(Rule {
                id,
                class_id,
                confidence,
                target,
                regex: Regex::new(pattern).expect("weak label regex must compile"),
            });
        };

        // Drums: prefer file stem matches, fall back to folder/path matches.
        push(
            "drums.kick.filename",
            "kick",
            0.92,
            MatchTarget::FileStem,
            r"(?i)\b(kick|bd|bass\s*drum)\b",
        );
        push(
            "drums.kick.path",
            "kick",
            0.85,
            MatchTarget::FullPath,
            r"(?i)\b(kicks|kick|bd|bass\s*drum)\b",
        );
        push(
            "drums.snare.filename",
            "snare",
            0.92,
            MatchTarget::FileStem,
            r"(?i)\b(snare|snr)\b",
        );
        push(
            "drums.snare.path",
            "snare",
            0.85,
            MatchTarget::FullPath,
            r"(?i)\b(snares|snare|snr)\b",
        );
        push(
            "drums.clap.filename",
            "clap",
            0.9,
            MatchTarget::FileStem,
            r"(?i)\b(clap|clps?)\b",
        );
        push(
            "drums.clap.path",
            "clap",
            0.8,
            MatchTarget::FullPath,
            r"(?i)\b(claps|clap)\b",
        );
        push(
            "drums.hihat_open.filename",
            "hihat_open",
            0.9,
            MatchTarget::FileStem,
            r"(?i)\b(open\s*hat|ohh)\b",
        );
        push(
            "drums.hihat_open.path",
            "hihat_open",
            0.8,
            MatchTarget::FullPath,
            r"(?i)\b(open\s*hats|open\s*hat|ohh)\b",
        );
        push(
            "drums.hihat_closed.filename",
            "hihat_closed",
            0.88,
            MatchTarget::FileStem,
            r"(?i)\b(closed\s*hat|chh)\b",
        );
        push(
            "drums.hihat_closed.path",
            "hihat_closed",
            0.78,
            MatchTarget::FullPath,
            r"(?i)\b(closed\s*hats|closed\s*hat|chh)\b",
        );
        push(
            "drums.hihat_generic.filename",
            "hihat",
            0.82,
            MatchTarget::FileStem,
            r"(?i)\b(hi\s*hat|hihat|hat)\b",
        );
        push(
            "drums.tom.filename",
            "tom",
            0.88,
            MatchTarget::FileStem,
            r"(?i)\b(tom|toms)\b",
        );
        push(
            "drums.rim.filename",
            "rimshot",
            0.88,
            MatchTarget::FileStem,
            r"(?i)\b(rim\s*shot|rimshot|rim)\b",
        );
        push(
            "drums.shaker.filename",
            "shaker",
            0.85,
            MatchTarget::FileStem,
            r"(?i)\b(shaker|shake)\b",
        );
        push(
            "drums.perc.path",
            "perc",
            0.75,
            MatchTarget::FullPath,
            r"(?i)\b(perc|percussion)\b",
        );
        push(
            "drums.crash.filename",
            "crash",
            0.88,
            MatchTarget::FileStem,
            r"(?i)\b(crash)\b",
        );
        push(
            "drums.ride.filename",
            "ride",
            0.88,
            MatchTarget::FileStem,
            r"(?i)\b(ride)\b",
        );
        push(
            "drums.cymbal.filename",
            "cymbal",
            0.8,
            MatchTarget::FileStem,
            r"(?i)\b(cymbal|cym)\b",
        );

        // Content types.
        push(
            "content.loop.path",
            "loop",
            0.7,
            MatchTarget::FullPath,
            r"(?i)\b(loop|loops)\b",
        );
        push(
            "content.oneshot.path",
            "one_shot",
            0.7,
            MatchTarget::FullPath,
            r"(?i)\b(one[\s_-]*shot|oneshot)\b",
        );
        push(
            "content.vocal.path",
            "vocal",
            0.7,
            MatchTarget::FullPath,
            r"(?i)\b(vocal|vox)\b",
        );
        push(
            "content.fx.filename",
            "fx",
            0.8,
            MatchTarget::FileStem,
            r"(?i)\b(efx|fx|sfx|effect|effects|impact|riser|rise|sweep|uplifter|downlifter)\b",
        );
        push(
            "content.fx.path",
            "fx",
            0.65,
            MatchTarget::FullPath,
            r"(?i)\b(efx|fx|sfx|effect|effects|impact|riser|rise|sweep|uplifter|downlifter)\b",
        );

        add_user_rules(&mut rules);
        rules
    })
}

pub fn weak_labels_for_relative_path(relative_path: &Path) -> Vec<WeakLabel> {
    let full_path = normalize_for_matching(relative_path);
    let file_stem = relative_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(normalize_str_for_matching)
        .unwrap_or_default();

    let mut best_by_class: HashMap<&'static str, WeakLabel> = HashMap::new();
    for rule in rules() {
        let haystack = match rule.target {
            MatchTarget::FileStem => file_stem.as_str(),
            MatchTarget::FullPath => full_path.as_str(),
        };
        if !rule.regex.is_match(haystack) {
            continue;
        }
        let candidate = WeakLabel {
            class_id: rule.class_id,
            confidence: rule.confidence,
            rule_id: rule.id,
        };
        match best_by_class.get(rule.class_id) {
            Some(existing) if existing.confidence >= candidate.confidence => {}
            _ => {
                best_by_class.insert(rule.class_id, candidate);
            }
        }
    }
    let mut labels: Vec<WeakLabel> = best_by_class.into_values().collect();
    if full_path.contains("bass drum") && labels.iter().any(|label| label.class_id == "kick") {
        labels.retain(|label| label.class_id != "bass");
    }
    labels.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    labels
}

fn add_user_rules(rules: &mut Vec<Rule>) {
    let Some(cfg) = crate::labeling::weak_config::load_label_rules_from_app_dir() else {
        return;
    };

    for (class_id, aliases) in cfg.categories {
        let Some(regex) = regex_for_aliases(&aliases) else {
            continue;
        };

        let class_id: &'static str = Box::leak(class_id.into_boxed_str());
        rules.push(Rule {
            id: Box::leak(format!("user.{class_id}.filename").into_boxed_str()),
            class_id,
            confidence: 0.93,
            target: MatchTarget::FileStem,
            regex: regex.clone(),
        });
        rules.push(Rule {
            id: Box::leak(format!("user.{class_id}.path").into_boxed_str()),
            class_id,
            confidence: 0.80,
            target: MatchTarget::FullPath,
            regex,
        });
    }
}

fn regex_for_aliases(aliases: &[String]) -> Option<Regex> {
    let mut parts: Vec<String> = aliases
        .iter()
        .map(|s| normalize_str_for_matching(s))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    parts.sort();
    parts.dedup();
    if parts.is_empty() {
        return None;
    }
    let alts = parts
        .into_iter()
        .map(|s| regex::escape(&s))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(r"(?i)\b(?:{alts})\b")).ok()
}

fn normalize_for_matching(path: &Path) -> String {
    normalize_str_for_matching(&path.to_string_lossy())
}

fn normalize_str_for_matching(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev: Option<char> = None;
    let mut iter = input.chars().peekable();

    while let Some(raw) = iter.next() {
        let ch = if raw == '\\' { '/' } else { raw };

        if !ch.is_alphanumeric() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            prev = None;
            continue;
        }

        if let Some(prev_ch) = prev {
            let alpha_digit_boundary = (prev_ch.is_alphabetic() && ch.is_numeric())
                || (prev_ch.is_numeric() && ch.is_alphabetic());
            let camel_boundary = prev_ch.is_lowercase() && ch.is_uppercase();
            let title_boundary = prev_ch.is_uppercase()
                && ch.is_uppercase()
                && iter.peek().is_some_and(|next| next.is_lowercase());
            if alpha_digit_boundary || camel_boundary || title_boundary {
                out.push(' ');
            }
        }

        out.push(ch);
        prev = Some(ch);
    }

    out.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn labels_kicks_from_filename() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Drums/Kicks/Big_Kick_01.wav"));
        assert!(labels.iter().any(|label| label.class_id == "kick"));
        let kick = labels.iter().find(|label| label.class_id == "kick").unwrap();
        assert_eq!(kick.rule_id, "drums.kick.filename");
        assert!(kick.confidence >= 0.9);
    }

    #[test]
    fn labels_open_hat_from_folder_name() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Loops/Open Hats/loop_120bpm.wav"));
        assert!(labels.iter().any(|label| label.class_id == "hihat_open"));
        let open_hat = labels
            .iter()
            .find(|label| label.class_id == "hihat_open")
            .unwrap();
        assert_eq!(open_hat.rule_id, "drums.hihat_open.path");
    }

    #[test]
    fn includes_ruleset_version_constant() {
        assert_eq!(WEAK_LABEL_RULESET_VERSION, 1);
    }

    #[test]
    fn labels_kick_with_digits() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Drums/Kicks/Kick10.aif"));
        assert!(labels.iter().any(|label| label.class_id == "kick"));
    }

    #[test]
    fn labels_kick_with_camelcase() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Drums/Kicks/BangKick.wav"));
        assert!(labels.iter().any(|label| label.class_id == "kick"));
    }

    #[test]
    fn labels_bass_drum_with_prefix() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Drums/Kicks/EBassDrum.wav"));
        assert!(labels.iter().any(|label| label.class_id == "kick"));
    }

    #[test]
    fn bass_drum_does_not_label_bass() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("Drums/Kicks/BassDrum01.wav"));
        assert!(labels.iter().any(|label| label.class_id == "kick"));
        assert!(!labels.iter().any(|label| label.class_id == "bass"));
    }

    #[test]
    fn labels_efx_as_fx() {
        let labels = weak_labels_for_relative_path(&PathBuf::from("FX/Noise/EfxRise01.wav"));
        assert!(labels.iter().any(|label| label.class_id == "fx"));
    }

    #[test]
    fn regex_for_aliases_matches_word_boundaries() {
        let re = super::regex_for_aliases(&["hh".to_string()]).unwrap();
        assert!(re.is_match("hh"));
        assert!(re.is_match("kick hh"));
        assert!(!re.is_match("shh"));
    }
}
