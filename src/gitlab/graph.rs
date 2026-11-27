//! Validation of the locally available commit graph.
use super::process::{GitError, GitReader};
use super::types::{Finding, LocalInput, ReasonCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphOk {
    pub base: String,
}

pub(crate) fn git_finding(error: GitError) -> Finding {
    Finding::new(match error {
        GitError::MissingObject => ReasonCode::MissingObject,
        GitError::LimitExceeded => ReasonCode::LimitExceeded,
        GitError::Failure | GitError::UnsupportedPlatform => ReasonCode::GitFailure,
    })
}

pub fn check_graph(input: &LocalInput, git: &dyn GitReader) -> Result<GraphOk, Finding> {
    let length = git.object_format().map_err(git_finding)?;
    for oid in [&input.base, &input.start, &input.head, &input.target] {
        if oid.len() != length
            || !oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Finding::new(ReasonCode::MissingObject));
        }
        if git.object_type(oid).map_err(git_finding)? != "commit" {
            return Err(Finding::new(ReasonCode::NotACommit));
        }
    }
    if git.is_shallow().map_err(git_finding)? {
        return Err(Finding::new(ReasonCode::ShallowRepository));
    }
    for (other, mismatch) in [
        (&input.start, ReasonCode::MergeBaseMismatch),
        (&input.target, ReasonCode::StaleTarget),
    ] {
        let bases = git.merge_bases(other, &input.head).map_err(git_finding)?;
        if bases.len() > 1 {
            return Err(Finding::new(ReasonCode::MultipleMergeBases));
        }
        if bases.len() != 1 || bases[0] != input.base {
            return Err(Finding::new(mismatch));
        }
    }
    Ok(GraphOk {
        base: input.base.clone(),
    })
}
