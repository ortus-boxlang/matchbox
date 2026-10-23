//! The CF parsing module (issue #45): reads ColdFusion source into the same
//! AST the BoxLang parser produces, so the existing compiler and VM run it.

use crate::ast::Statement;
use crate::parser::ParseError;
use crate::tokenizer::Span;
use anyhow::Result;
use std::path::Path;

mod transpile;
pub use transpile::transpile;

/// Which front end reads a source file, chosen by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// `.bx`, `.bxs`, and anything unknown.
    BoxLangScript,
    /// `.bxm`
    BoxLangTemplate,
    /// `.cfc`, `.cfs`
    CfScript,
    /// `.cfm`, `.cfml`
    CfTemplate,
}

impl SourceKind {
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("bxm") => Self::BoxLangTemplate,
            Some("cfm") | Some("cfml") => Self::CfTemplate,
            Some("cfc") | Some("cfs") => Self::CfScript,
            _ => Self::BoxLangScript,
        }
    }

    pub fn is_template(self) -> bool {
        matches!(self, Self::BoxLangTemplate | Self::CfTemplate)
    }
}

/// The one entry point every caller uses to turn a source file into MatchBox's AST.
pub fn parse_source(
    kind: SourceKind,
    source: &str,
    filename: Option<&str>,
) -> Result<Vec<Statement>> {
    match kind {
        SourceKind::BoxLangScript => crate::parser::parse(source, filename),
        SourceKind::BoxLangTemplate => crate::parser::parse_bxm(source, filename),
        SourceKind::CfTemplate => {
            let mut ast =
                crate::parser::template::parse_template_with_prefix(source, filename, "cf")?;
            transpile(&mut ast);
            Ok(ast)
        }
        SourceKind::CfScript => {
            reject_tag_based_script(source, filename)?;
            let mut ast = crate::parser::parse_cf_script(source, filename)?;
            transpile(&mut ast);
            Ok(ast)
        }
    }
}

/// CFScript can never start with `<`, so a .cfc/.cfs that does is tag-based CF
/// (`<cfcomponent>`), which is not supported. Report that instead of a confusing
/// parse or runtime error.
fn reject_tag_based_script(source: &str, filename: Option<&str>) -> Result<()> {
    let start = source.len() - source.trim_start().len();
    if !source[start..].starts_with('<') {
        return Ok(());
    }
    let before = &source[..start];
    let line = 1 + before.matches('\n').count() as u32;
    let col = 1 + (start - before.rfind('\n').map_or(0, |i| i + 1)) as u32;
    let span = Span {
        start,
        end: start + 1,
        line,
        col,
    };
    Err(ParseError::new(
        filename,
        span,
        "tag-based CF (such as <cfcomponent>) is not supported in .cfc/.cfs files; use script syntax: component { ... }",
    )
    .with_source(source)
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_from_extension() {
        let cases = [
            ("page.cfm", SourceKind::CfTemplate),
            ("PAGE.CFM", SourceKind::CfTemplate),
            ("page.cfml", SourceKind::CfTemplate),
            ("User.cfc", SourceKind::CfScript),
            ("job.cfs", SourceKind::CfScript),
            ("page.bxm", SourceKind::BoxLangTemplate),
            ("main.bxs", SourceKind::BoxLangScript),
            ("User.bx", SourceKind::BoxLangScript),
            ("noext", SourceKind::BoxLangScript),
        ];
        for (path, kind) in cases {
            assert_eq!(SourceKind::from_path(Path::new(path)), kind, "path: {path}");
        }
    }

    #[test]
    fn cfml_parses_like_cfm() {
        let src = "<cfset x = 1><cfoutput>#x#</cfoutput>";
        let cfml = parse_source(SourceKind::from_path(Path::new("page.cfml")), src, None).unwrap();
        let cfm = parse_source(SourceKind::from_path(Path::new("page.cfm")), src, None).unwrap();
        assert_eq!(cfml, cfm);
    }

    #[test]
    fn tag_based_components_report_a_clear_error() {
        for src in [
            "<cfcomponent>\n</cfcomponent>",
            "\n  <!--- note --->\n<cfcomponent>",
        ] {
            let err = parse_source(SourceKind::CfScript, src, Some("User.cfc")).unwrap_err();
            assert!(
                err.to_string().contains("tag-based CF"),
                "source: {src:?}, error: {err}"
            );
        }
        assert!(parse_source(SourceKind::CfScript, "component { }", Some("User.cfc")).is_ok());
    }
}
