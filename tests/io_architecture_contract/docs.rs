const IO_ARCHITECTURE_TARGET: &str = include_str!("../../docs/IO_ARCHITECTURE_TARGET.md");
const IO_ALIGNMENT_ESTIMATE: &str = include_str!("../../docs/IO_ALIGNMENT_ESTIMATE.md");

const OWNER_FORBIDDEN_SIDE_EFFECT_PAIRS: &[(&str, &str)] = &[
    ("**I/O coordinator**", "Direct filesystem or SQL work."),
    (
        "**Durable app-local journal**",
        "Source manifest truth or arbitrary user metadata.",
    ),
    (
        "**File operation owner**",
        "SQLite transactions or browser projection.",
    ),
    (
        "**Per-physical-source DB writer owner**",
        "Filesystem traversal, copy, hashing, cache payload writes, or another source.",
    ),
    ("**Global-library owner**", "physical file mutation"),
    ("**Harvest owner**", "Rendering or copying bytes."),
    (
        "**Projection publisher**",
        "Filesystem/SQLite reads during UI application.",
    ),
    (
        "**Artifact store**",
        "Durable user metadata or source membership.",
    ),
];

#[test]
fn source_revision_contract_is_one_monotonic_cursor() {
    for required in [
        concat!(
            "For the ",
            "\x600.19.1\x60",
            " target, each physical source has one monotonic committed ",
            "\x60SourceRevision\x60",
            ".",
        ),
        "It is the sole authoritative publication cursor for source membership, path, and structural\ndirectory truth.",
        "A directory generation is only a staging/readiness aid fenced to the committed\n\x60SourceRevision\x60;",
        "There is no composite source-publication cursor.",
        "advances the single source revision only when authoritative source truth changed,",
    ] {
        assert!(
            IO_ARCHITECTURE_TARGET.contains(required),
            "IO_ARCHITECTURE_TARGET.md must preserve the resolved single-cursor contract; missing required wording: {required}"
        );
    }
    assert!(
        IO_ALIGNMENT_ESTIMATE.contains("The OPT-1298 contract gate"),
        "IO_ALIGNMENT_ESTIMATE.md must record the bounded OPT-1298 contract gate"
    );
    assert!(
        !IO_ARCHITECTURE_TARGET.contains("Should source revisions be one global manifest sequence"),
        "the old open source-revision decision must be removed once the 0.19.1 contract is resolved"
    );
}

#[test]
fn io_target_names_owners_and_forbidden_side_effects() {
    for (owner, forbidden) in OWNER_FORBIDDEN_SIDE_EFFECT_PAIRS {
        let row = IO_ARCHITECTURE_TARGET
            .lines()
            .find(|line| line.starts_with('|') && line.contains(owner))
            .unwrap_or_else(|| {
                panic!("IO_ARCHITECTURE_TARGET.md must contain an owner-table row for {owner}")
            });
        assert!(
            row.contains(forbidden),
            "owner row for {owner} must contain paired forbidden-side-effect text {forbidden}; row was: {row}"
        );
    }
}
