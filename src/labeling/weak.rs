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
            "content.fx.path",
            "fx",
            0.65,
            MatchTarget::FullPath,
            r"(?i)\b(fx|sfx|effect|effects|impact|riser|rise|sweep|uplifter|downlifter)\b",
        );

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
    labels.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    labels
}

fn normalize_for_matching(path: &Path) -> String {
    normalize_str_for_matching(&path.to_string_lossy())
}

fn normalize_str_for_matching(input: &str) -> String {
    input
        .replace('\\', "/")
        .chars()
        .map(|ch| match ch {
            '_' | '-' | '.' => ' ',
            other => other,
        })
        .collect::<String>()
        .to_lowercase()
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
}

