//! Size and cognitive complexity of a Python function body.
//!
//! Definitions: `docs/dev/complex_function_docstring/03b_complexity_gate.md` (D-gate2, D-gate3).

use super::docstrings::docstring_of;
use super::functions::{PythonFunctionSignature, PythonParameterKind};
use crate::code_lint::ast::tree::ParsedFile;
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_except_handler, walk_expr, walk_match_case, walk_stmt,
};
use ruff_python_ast::{BoolOp, ExceptHandler, Expr, Stmt};

impl PythonFunctionSignature<'_> {
    /// The number of statements in the function's own body, docstring excluded.
    ///
    /// Every statement counts 1, compound statements included, and so does each `elif` clause.
    /// The bodies of compound statements (branches, loops and their `else`, `try` handlers,
    /// `else` and `finally`, `with`, `match` cases) are counted too; a nested `def` or `class`
    /// counts 1 and its body is not entered.
    #[must_use]
    pub(in crate::code_lint) fn statement_count(&self) -> usize {
        let body = &self.definition.body;
        let own_body = if self.docstring().is_some() {
            &body[1..]
        } else {
            body
        };
        count_statements(own_body)
    }

    /// The cognitive complexity of the function, following the Cognitive Complexity white paper.
    ///
    /// `if`, ternaries, loops, `except` clauses and `match` add 1 plus the current nesting;
    /// `elif` and every `else` (of `if`, loops and `try`) add 1; each run of like boolean
    /// operators adds 1; a call to the function itself (`self.<name>` / `cls.<name>` in a
    /// method) adds 1 once. Nested functions, lambdas and classes add nothing but raise the
    /// nesting of their body, except a function whose body is one nested `def` plus
    /// `return <name>` statements (a decorator), whose nested `def` keeps the nesting.
    /// Comprehensions, `try`, `finally` and `with` add nothing.
    #[must_use]
    pub(in crate::code_lint) fn cognitive_complexity(&self) -> usize {
        let receiver = self
            .parameters
            .first()
            .filter(|parameter| parameter.kind == PythonParameterKind::Receiver)
            .map(|parameter| parameter.name.as_str());
        let mut visitor = CognitiveComplexityVisitor {
            file: self.name_node.file,
            function_name: &self.name,
            receiver,
            nesting: 0,
            nested_scope_depth: 0,
            in_decorator_body: false,
            enclosing_bool_op: None,
            complexity: 0,
            calls_itself: false,
        };
        visitor.visit_function_body(&self.definition.body);
        visitor.complexity + usize::from(visitor.calls_itself)
    }
}

/// The statements of `body` and of the bodies of its compound statements, as
/// [`PythonFunctionSignature::statement_count`].
fn count_statements(body: &[Stmt]) -> usize {
    body.iter()
        .map(|statement| {
            1 + match statement {
                Stmt::If(if_statement) => {
                    count_statements(&if_statement.body)
                        + if_statement
                            .elif_else_clauses
                            .iter()
                            .map(|clause| {
                                usize::from(clause.test.is_some()) + count_statements(&clause.body)
                            })
                            .sum::<usize>()
                }
                Stmt::For(for_statement) => {
                    count_statements(&for_statement.body) + count_statements(&for_statement.orelse)
                }
                Stmt::While(while_statement) => {
                    count_statements(&while_statement.body)
                        + count_statements(&while_statement.orelse)
                }
                Stmt::Try(try_statement) => {
                    count_statements(&try_statement.body)
                        + try_statement
                            .handlers
                            .iter()
                            .map(|ExceptHandler::ExceptHandler(handler)| {
                                count_statements(&handler.body)
                            })
                            .sum::<usize>()
                        + count_statements(&try_statement.orelse)
                        + count_statements(&try_statement.finalbody)
                }
                Stmt::With(with_statement) => count_statements(&with_statement.body),
                Stmt::Match(match_statement) => match_statement
                    .cases
                    .iter()
                    .map(|case| count_statements(&case.body))
                    .sum(),
                _ => 0,
            }
        })
        .sum()
}

/// Accumulates the cognitive complexity of one function body.
struct CognitiveComplexityVisitor<'a> {
    file: &'a ParsedFile,
    function_name: &'a str,
    /// The method receiver (`self` / `cls`), if the function has one.
    receiver: Option<&'a str>,
    nesting: usize,
    /// Depth of the nested `def`, `lambda` and `class` scopes being walked.
    nested_scope_depth: usize,
    /// True while walking a function body that is one nested `def` plus `return <name>`.
    in_decorator_body: bool,
    /// The operator of the boolean operation whose direct operand is being visited.
    enclosing_bool_op: Option<BoolOp>,
    complexity: usize,
    calls_itself: bool,
}

impl<'a> CognitiveComplexityVisitor<'a> {
    fn visit_function_body(&mut self, body: &'a [Stmt]) {
        let enclosing = self.in_decorator_body;
        self.in_decorator_body = self.is_decorator_body(body);
        self.visit_body(body);
        self.in_decorator_body = enclosing;
    }

    /// True if `body`, docstring aside, is one `def` plus `return <name>` statements.
    fn is_decorator_body(&self, body: &[Stmt]) -> bool {
        let own_body = if docstring_of(body, self.file).is_some() {
            &body[1..]
        } else {
            body
        };
        let (definitions, others): (Vec<&Stmt>, Vec<&Stmt>) = own_body
            .iter()
            .partition(|statement| matches!(statement, Stmt::FunctionDef(_)));
        definitions.len() == 1
            && others.iter().all(|statement| {
                matches!(
                    statement,
                    Stmt::Return(return_statement)
                        if matches!(return_statement.value.as_deref(), Some(Expr::Name(_)))
                )
            })
    }

    /// Adds the structural increment of a construct at the current nesting.
    const fn add_nested_increment(&mut self) {
        self.complexity += 1 + self.nesting;
    }

    /// Visits the `else` clause of a loop or `try`, which adds 1 and raises the nesting.
    fn visit_else_clause(&mut self, orelse: &'a [Stmt]) {
        if !orelse.is_empty() {
            self.complexity += 1;
            self.nested(|visitor| visitor.visit_body(orelse));
        }
    }

    /// Runs `visit` one nesting level deeper.
    fn nested(&mut self, visit: impl FnOnce(&mut Self)) {
        self.nesting += 1;
        visit(self);
        self.nesting -= 1;
    }

    /// Runs `visit` inside a nested `def`, `lambda` or `class` scope, `nesting_increment` levels
    /// deeper.
    fn nested_scope(&mut self, nesting_increment: usize, visit: impl FnOnce(&mut Self)) {
        self.nesting += nesting_increment;
        self.nested_scope_depth += 1;
        visit(self);
        self.nested_scope_depth -= 1;
        self.nesting -= nesting_increment;
    }

    /// True if `callee` names the function itself: `<name>`, or `<receiver>.<name>` in a method.
    fn is_own_callee(&self, callee: &Expr) -> bool {
        match (callee, self.receiver) {
            (Expr::Name(name), None) => name.id == self.function_name,
            (Expr::Attribute(attribute), Some(receiver)) => {
                attribute.attr.id == self.function_name
                    && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id == receiver)
            }
            _ => false,
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for CognitiveComplexityVisitor<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::If(if_statement) => {
                self.add_nested_increment();
                self.visit_expr(&if_statement.test);
                self.nested(|visitor| visitor.visit_body(&if_statement.body));
                for clause in &if_statement.elif_else_clauses {
                    self.complexity += 1;
                    if let Some(test) = &clause.test {
                        self.visit_expr(test);
                    }
                    self.nested(|visitor| visitor.visit_body(&clause.body));
                }
            }
            Stmt::For(for_statement) => {
                self.add_nested_increment();
                self.visit_expr(&for_statement.target);
                self.visit_expr(&for_statement.iter);
                self.nested(|visitor| visitor.visit_body(&for_statement.body));
                self.visit_else_clause(&for_statement.orelse);
            }
            Stmt::While(while_statement) => {
                self.add_nested_increment();
                self.visit_expr(&while_statement.test);
                self.nested(|visitor| visitor.visit_body(&while_statement.body));
                self.visit_else_clause(&while_statement.orelse);
            }
            Stmt::Try(try_statement) => {
                self.visit_body(&try_statement.body);
                for handler in &try_statement.handlers {
                    self.add_nested_increment();
                    self.nested(|visitor| walk_except_handler(visitor, handler));
                }
                self.visit_else_clause(&try_statement.orelse);
                self.visit_body(&try_statement.finalbody);
            }
            Stmt::Match(match_statement) => {
                self.add_nested_increment();
                self.visit_expr(&match_statement.subject);
                self.nested(|visitor| {
                    for case in &match_statement.cases {
                        walk_match_case(visitor, case);
                    }
                });
            }
            Stmt::FunctionDef(function_def) => {
                for decorator in &function_def.decorator_list {
                    self.visit_decorator(decorator);
                }
                self.visit_parameters(&function_def.parameters);
                let nesting_increment = usize::from(!self.in_decorator_body);
                self.nested_scope(nesting_increment, |visitor| {
                    visitor.visit_function_body(&function_def.body);
                });
            }
            Stmt::ClassDef(class_def) => {
                for decorator in &class_def.decorator_list {
                    self.visit_decorator(decorator);
                }
                if let Some(arguments) = &class_def.arguments {
                    self.visit_arguments(arguments);
                }
                self.nested_scope(1, |visitor| {
                    let enclosing = visitor.in_decorator_body;
                    visitor.in_decorator_body = false;
                    visitor.visit_body(&class_def.body);
                    visitor.in_decorator_body = enclosing;
                });
            }
            _ => walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        let enclosing_bool_op = self.enclosing_bool_op.take();
        match expr {
            Expr::BoolOp(bool_op) => {
                if enclosing_bool_op != Some(bool_op.op) {
                    self.complexity += 1;
                }
                for operand in &bool_op.values {
                    self.enclosing_bool_op = Some(bool_op.op);
                    self.visit_expr(operand);
                }
            }
            Expr::If(_) => {
                self.add_nested_increment();
                self.nested(|visitor| walk_expr(visitor, expr));
            }
            Expr::Lambda(lambda) => {
                if let Some(parameters) = &lambda.parameters {
                    self.visit_parameters(parameters);
                }
                self.nested_scope(1, |visitor| visitor.visit_expr(&lambda.body));
            }
            Expr::Call(call) => {
                if self.nested_scope_depth == 0 && self.is_own_callee(&call.func) {
                    self.calls_itself = true;
                }
                walk_expr(self, expr);
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::functions::extract_function_signatures;
    use super::*;
    use crate::diagnostic::Language;
    use indoc::indoc;
    use rstest::rstest;

    /// The statement count and cognitive complexity of the first function in `source`.
    fn measure(source: &str) -> (usize, usize) {
        let file = ParsedFile::new(source, Language::Python);
        let signature = &extract_function_signatures(&file)[0];
        (
            signature.statement_count(),
            signature.cognitive_complexity(),
        )
    }

    #[rstest]
    #[case::simple_statements(indoc! {"
        def f():
            x = 1
            print(x)
            return x
    "}, 3)]
    #[case::docstring_excluded(indoc! {r#"
        def f():
            """Doc."""
            return 1
    "#}, 1)]
    #[case::if_elif_else(indoc! {"
        def f(a, b):
            if a:
                x = 1
            elif b:
                x = 2
            else:
                x = 3
    "}, 5)]
    #[case::loop_else(indoc! {"
        def f(items):
            for item in items:
                print(item)
            else:
                return None
    "}, 3)]
    #[case::try_clauses(indoc! {"
        def f():
            try:
                a()
            except ValueError:
                b()
            else:
                c()
            finally:
                d()
    "}, 5)]
    #[case::with_and_while(indoc! {"
        def f(lock):
            with lock:
                while True:
                    break
    "}, 3)]
    #[case::match_cases(indoc! {"
        def f(x):
            match x:
                case 1:
                    a()
                case _:
                    pass
    "}, 3)]
    #[case::nested_def_and_class_count_one(indoc! {"
        def f():
            def g():
                a()
                b()
            class C:
                x = 1
                y = 2
            return g
    "}, 3)]
    fn statement_count(#[case] source: &str, #[case] expected: usize) {
        assert_eq!(measure(source).0, expected);
    }

    #[rstest]
    #[case::straight_line(indoc! {"
        def f(x):
            y = x + 1
            return y
    "}, 0)]
    #[case::if_elif_else(indoc! {"
        def f(a, b):
            if a:
                return 1
            elif b:
                return 2
            else:
                return 3
    "}, 3)]
    #[case::nested_if_adds_nesting(indoc! {"
        def f(a, b):
            if a:
                if b:
                    return 1
    "}, 3)]
    #[case::elif_and_else_ignore_nesting(indoc! {"
        def f(items):
            for item in items:
                if item:
                    pass
                elif item is None:
                    pass
                else:
                    pass
    "}, 5)]
    #[case::loops(indoc! {"
        async def f(items, stream):
            for item in items:
                pass
            async for chunk in stream:
                pass
            while items:
                items.pop()
    "}, 3)]
    #[case::loop_else(indoc! {"
        def f(items):
            for item in items:
                break
            else:
                return None
    "}, 2)]
    #[case::except_clauses_and_try_else(indoc! {"
        def f():
            try:
                a()
            except ValueError:
                pass
            except (KeyError, TypeError):
                pass
            else:
                b()
            finally:
                c()
    "}, 3)]
    #[case::except_adds_nesting(indoc! {"
        def f(items):
            for item in items:
                try:
                    item()
                except ValueError:
                    pass
    "}, 3)]
    #[case::match_counts_once(indoc! {"
        def f(x):
            match x:
                case 1:
                    pass
                case 2:
                    pass
                case _:
                    pass
    "}, 1)]
    #[case::match_cases_and_guards_raise_nesting(indoc! {"
        def f(x, y):
            match x:
                case 1 if (y if x else None):
                    if y:
                        pass
    "}, 5)]
    #[case::ternary(indoc! {"
        def f(a):
            return 1 if a else 2
    "}, 1)]
    #[case::ternary_raises_nesting(indoc! {"
        def f(a, b):
            return 1 if a else (2 if b else 3)
    "}, 3)]
    #[case::boolean_run(indoc! {"
        def f(a, b, c):
            return a and b and c
    "}, 1)]
    #[case::boolean_runs_in_any_context(indoc! {"
        def f(a, b, c, d, e, g):
            print(a and b and c or d or e and g)
    "}, 3)]
    #[case::negation_starts_a_new_run(indoc! {"
        def f(a, b, c):
            return a and not (b and c)
    "}, 2)]
    #[case::with_try_finally_and_comprehensions_add_nothing(indoc! {"
        def f(lock, items):
            with lock:
                try:
                    return [item for item in items if item]
                finally:
                    pass
    "}, 0)]
    #[case::lambda_raises_nesting(indoc! {"
        def f():
            return lambda x: 1 if x else 2
    "}, 2)]
    #[case::nested_def_raises_nesting(indoc! {"
        def f(a):
            def g():
                if a:
                    pass
            g()
            return g
    "}, 2)]
    #[case::nested_class_methods_raise_nesting_twice(indoc! {"
        def f(a):
            class C:
                def m(self):
                    if a:
                        pass
            return C
    "}, 3)]
    #[case::decorator_keeps_nesting(indoc! {"
        def deco(func):
            def wrapper(*args):
                if args:
                    return func(*args)
            return wrapper
    "}, 1)]
    #[case::decorator_exemption_chains(indoc! {r#"
        def deco_factory(flag):
            """Doc."""
            def deco(func):
                def wrapper(*args):
                    if flag:
                        return func(*args)
                return wrapper
            return deco
    "#}, 1)]
    #[case::recursion(indoc! {"
        def fact(n):
            return n * fact(n - 1) + fact(0)
    "}, 1)]
    #[case::recursion_through_receiver(indoc! {"
        class Tree:
            def walk(self, node):
                return self.walk(node.child)
    "}, 1)]
    #[case::bare_call_in_method_is_not_recursion(indoc! {"
        class Tree:
            def walk(self, node):
                return walk(node)
    "}, 0)]
    #[case::recursion_in_nested_scope_ignored(indoc! {"
        def fact(n):
            callback = lambda: fact(n)
            return callback
    "}, 0)]
    fn cognitive_complexity(#[case] source: &str, #[case] expected: usize) {
        assert_eq!(measure(source).1, expected);
    }
}
