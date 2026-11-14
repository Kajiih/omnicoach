//! Semantic analysis engines built on top of the syntax adapters.

architecture_component!(CodeLintSemantic);

mod bindings;
mod calls;
mod comments;
mod type_annotations;

pub(super) use self::comments::{
    OmniDirectivePrefix, is_line_prefix_whitespace, is_line_suffix_whitespace,
    is_standalone_comment_span, may_contain_suppression_directive, parse_omni_directive_prefix,
    split_python_directive_segments, starts_with_tool_directive,
};

pub use self::bindings::{
    SuffixedBindingMatch, collect_imported_names, collect_renameable_bindings,
    find_suffixed_bindings,
};
pub use self::calls::{CallMatch, find_banned_calls};
pub use self::comments::{CommentIndex, strip_comment_delimiters};
pub use self::type_annotations::find_banned_type_annotations;
