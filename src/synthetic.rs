//! Explicit development-only reader. Not a substitute for ValidatedSnapshot.
use crate::{Error, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
#[path = "validation.rs"]
mod validation;
use validation::{address, Source};
fn fail<T>(message: &str) -> Result<T> {
    Err(Error(message.into()))
}
/// Development output is deliberately not typed as a production validated snapshot.
pub struct SyntheticSnapshot {
    meta: Value,
    sources: BTreeMap<String, Source>,
}
impl SyntheticSnapshot {
    pub fn open(path: &Path, expected_id: &str) -> Result<Self> {
        let (meta, sources) = validation::read_fixture(path, expected_id)?;
        Ok(Self { meta, sources })
    }
    pub fn metadata(&self) -> Value {
        self.meta.clone()
    }
    pub fn source(&self, snapshot_id: &str, path: &str, start: u64, end: u64) -> Result<Value> {
        if self.meta["snapshot_id"].as_str() != Some(snapshot_id) {
            return fail("snapshot mismatch");
        }
        address(path)?;
        if start == 0 || start > end || end - start >= 100 {
            return fail("invalid interval");
        }
        let s = self
            .sources
            .get(path)
            .ok_or_else(|| Error("unknown address".into()))?;
        if end > s.positions.len() as u64 {
            return fail("interval not found");
        }
        let rows = &s.positions[(start - 1) as usize..end as usize];
        let a = rows[0][1] as usize;
        let b = rows[rows.len() - 1][2] as usize;
        if b - a > 16384 {
            return fail("byte limit exceeded");
        }
        let text = std::str::from_utf8(&s.raw[a..b]).map_err(|e| Error(e.to_string()))?;
        Ok(
            json!({"path":path,"text":text,"sha256":s.sha256,"oid":s.oid,"positions":rows,"start_byte":a,"end_byte":b}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_physical_lines_and_bytes() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "Long.bsl".into(),
            Source {
                raw: vec![b'x'; 16385],
                oid: "fixture".into(),
                sha256: "fixture".into(),
                positions: vec![[1, 0, 16385]],
            },
        );
        sources.insert(
            "Many.bsl".into(),
            Source {
                raw: vec![b'\n'; 101],
                oid: "fixture".into(),
                sha256: "fixture".into(),
                positions: (0..101).map(|i| [i + 1, i, i + 1]).collect(),
            },
        );
        let snapshot = SyntheticSnapshot {
            meta: json!({"snapshot_id":"unit-fixture"}),
            sources,
        };
        assert!(snapshot.source("unit-fixture", "Long.bsl", 1, 1).is_err());
        assert!(snapshot.source("unit-fixture", "Many.bsl", 1, 100).is_ok());
        assert!(snapshot.source("unit-fixture", "Many.bsl", 1, 101).is_err());
    }
}
