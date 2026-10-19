"""Exemption mutations for `uninherited-protocol` (`docs/dev/protocol_inheritance/03_design.md`)."""

from __future__ import annotations

from .model import CLASS_DECLARATIONS, Cluster, Mutation, Unmutated

UNINHERITED_PROTOCOL = "src/code_lint/rules/uninherited_protocol.rs"

CLUSTER = Cluster(
    rules=("uninherited_protocol",),
    mutations=(
        Mutation(
            label="class inheriting from the Protocol is not flagged (E1a)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    "        .filter(|protocol| {\n"
                    "            !ancestor_names.contains(classes[protocol.index].declaration().name.as_str())\n"
                    "        })\n",
                    "",
                ),
            ),
            cases=(
                "uninherited_protocol::pass::inherits_protocol",
                "uninherited_protocol::pass::inherits_generic_protocol",
            ),
        ),
        Mutation(
            label="class inheriting from the Protocol through an in-file base is not flagged (E1b)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    "local_ancestors(classes, index)",
                    "crate::code_lint::ast::python::local_base_classes(classes, index)",
                ),
            ),
            cases=("uninherited_protocol::pass::inherits_protocol_through_local_base",),
        ),
        Mutation(
            label="Protocol classes are not checked (E2a)",
            path=UNINHERITED_PROTOCOL,
            edits=(("decl.is_protocol() || decl.is_abc()", "decl.is_abc()"),),
            cases=("uninherited_protocol::pass::protocol_redeclaring_methods",),
        ),
        Mutation(
            label="ABC classes are not checked (E2b)",
            path=UNINHERITED_PROTOCOL,
            edits=(("decl.is_protocol() || decl.is_abc()", "decl.is_protocol()"),),
            cases=("uninherited_protocol::pass::abc_declaring_methods",),
        ),
        Mutation(
            label="Enum classes are not checked (E3a)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                ("decl.is_enum() || decl.is_named_tuple()", "decl.is_named_tuple()"),
            ),
            cases=("uninherited_protocol::pass::enum_defining_methods",),
        ),
        Mutation(
            label="NamedTuple classes are not checked (E3c)",
            path=UNINHERITED_PROTOCOL,
            edits=(("decl.is_enum() || decl.is_named_tuple()", "decl.is_enum()"),),
            cases=("uninherited_protocol::pass::named_tuple_defining_methods",),
        ),
        Mutation(
            label="Protocol with fewer than `min-methods` methods is not checked (E4)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    ".filter(|protocol| protocol.methods.len() >= min_methods)",
                    ".filter(|protocol| protocol.methods.len() >= 1)",
                ),
            ),
            cases=("uninherited_protocol::pass::protocol_below_min_methods",),
        ),
        Mutation(
            label="private and dunder methods are not counted (E5)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    ".filter(|method| !method.name.starts_with('_'))",
                    ".filter(|_| true)",
                ),
            ),
            cases=(
                "uninherited_protocol::pass::underscore_prefixed_methods_not_counted",
            ),
        ),
        Mutation(
            label="property accessors are not counted, on the Protocol or the class (E6)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    ".filter(|method| !properties.contains(method.name.as_str()))",
                    ".filter(|_| true)",
                ),
            ),
            cases=(
                "uninherited_protocol::pass::protocol_property_not_counted",
                "uninherited_protocol::pass::property_not_counted_as_implementation",
            ),
        ),
        Mutation(
            label="a base named `Protocol` outside `typing` is not a Protocol (E7)",
            path=CLASS_DECLARATIONS,
            edits=(
                (
                    ".any(|path| PROTOCOL_PATHS.contains(&path.as_str()))",
                    '.any(|path| path.ends_with("Protocol"))',
                ),
            ),
            cases=("uninherited_protocol::pass::non_typing_protocol_base",),
        ),
        Mutation(
            label="a Protocol extended by another matched Protocol is not reported (E8)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    "    matched.retain(|protocol| !extended.contains(&protocol.index));\n",
                    "",
                ),
            ),
            cases=("uninherited_protocol::fail::most_derived_protocol_reported",),
        ),
        Mutation(
            label="bare Fake* classes are left to fake-without-protocol (E9)",
            path=UNINHERITED_PROTOCOL,
            edits=(
                (
                    "let is_bare_fake = is_fake_class_name(&decl.name)",
                    "let is_bare_fake = false && is_fake_class_name(&decl.name)",
                ),
            ),
            cases=(
                "uninherited_protocol::pass::bare_fake_left_to_fake_without_protocol",
            ),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layout for uninherited-protocol",
            reason="No exemption: the class does not define every method of the Protocol.",
            cases=("uninherited_protocol::pass::missing_protocol_method",),
        ),
    ),
)
