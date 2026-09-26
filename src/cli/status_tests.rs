use super::*;

#[test]
fn test_freshness_head_newer_is_stale() {
    // HEAD committed after the last index run => stale.
    let f = Freshness::compute(2_000, Some(1_000));
    assert!(f.stale);
    assert_eq!(f.head_commit_time, 2_000);
    assert_eq!(f.last_indexed, Some(1_000));
}

#[test]
fn test_freshness_head_older_is_fresh() {
    // No commits since the last index (quiet repo) => not stale. This is
    // the false-positive guard: age alone must not flag an idle repo.
    let f = Freshness::compute(1_000, Some(2_000));
    assert!(!f.stale);
}

#[test]
fn test_freshness_equal_is_fresh() {
    let f = Freshness::compute(1_000, Some(1_000));
    assert!(!f.stale);
}

#[test]
fn test_freshness_never_indexed_is_stale() {
    let f = Freshness::compute(1_000, None);
    assert!(f.stale);
    assert_eq!(f.last_indexed, None);
}

#[test]
fn test_format_duration_units() {
    assert_eq!(format_duration(30), "30s");
    assert_eq!(format_duration(120), "2m");
    assert_eq!(format_duration(7_200), "2h");
    assert_eq!(format_duration(172_800), "2d");
    assert_eq!(format_duration(-5), "0s");
}
