// Helpers for predictable version bumps shared between the build script and tests.

use semver::Version;

/// Increment the minor component of a semver string and reset the patch.
pub fn bump_minor(version: &str) -> Result<String, String> {
    let mut parsed =
        Version::parse(version).map_err(|error| format!("Invalid version '{version}': {error}"))?;
    parsed.minor += 1;
    parsed.patch = 0;
    Ok(parsed.to_string())
}
