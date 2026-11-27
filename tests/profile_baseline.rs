// Captured from 3b02a450e3a67cf555192797a27ccf8537cf7b0c before the refactor.
#[test]
fn baseline_has_one_row_per_profile() {
    let rows: Vec<_> = include_str!("../src/snapshot_v5/profile_baseline.tsv")
        .lines()
        .collect();
    assert_eq!(rows.len(), 38);
    for row in rows {
        assert_eq!(row.split('\t').count(), 6);
    }
}
