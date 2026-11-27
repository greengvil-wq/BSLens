//! Validation of the locally available commit graph.
use super::process::{GitError, GitReader};
use super::types::{Finding, LocalInput, ReasonCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphOk {
    pub base: String,
}

pub(crate) fn git_finding(error: GitError) -> Finding {
    finding(match error {
        GitError::MissingObject => ReasonCode::MissingObject,
        GitError::LimitExceeded => ReasonCode::LimitExceeded,
        GitError::Failure | GitError::UnsupportedPlatform => ReasonCode::GitFailure,
    })
}

fn finding(reason: ReasonCode) -> Finding {
    let mut result = Finding::new(reason);
    result.evidence.rule_id = Some("R3".into());
    result
}
fn object_finding(reason: ReasonCode, oid: &str) -> Finding {
    let mut result = finding(reason);
    result.evidence.object_oid = Some(oid.into());
    result
}
fn bases_finding(reason: ReasonCode, base: &str, bases: Vec<String>) -> Finding {
    let mut result = finding(reason);
    result.evidence.base_oid = Some(base.into());
    result.evidence.merge_bases = bases;
    result
}

pub fn check_graph(input: &LocalInput, git: &dyn GitReader) -> Result<GraphOk, Finding> {
    let length = git.object_format().map_err(git_finding)?;
    for oid in [&input.base, &input.start, &input.head, &input.target] {
        if oid.len() != length
            || !oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(object_finding(ReasonCode::MissingObject, oid));
        }
        if git.object_type(oid).map_err(|e| {
            if e == GitError::MissingObject {
                object_finding(ReasonCode::MissingObject, oid)
            } else {
                git_finding(e)
            }
        })? != "commit"
        {
            return Err(object_finding(ReasonCode::NotACommit, oid));
        }
    }
    if git.is_shallow().map_err(git_finding)? {
        return Err(finding(ReasonCode::ShallowRepository));
    }
    for (other, mismatch) in [
        (&input.start, ReasonCode::MergeBaseMismatch),
        (&input.target, ReasonCode::StaleTarget),
    ] {
        let bases = git.merge_bases(other, &input.head).map_err(git_finding)?;
        if bases.len() > 1 {
            return Err(bases_finding(
                ReasonCode::MultipleMergeBases,
                &input.base,
                bases,
            ));
        }
        if bases.len() != 1 || bases[0] != input.base {
            return Err(bases_finding(mismatch, &input.base, bases));
        }
    }
    Ok(GraphOk {
        base: input.base.clone(),
    })
}
