Jsonnet Specification
=====================

This page is the authority on what Jsonnet programs should do. It defines Jsonnet lexing and syntax. It describes which programs should be rejected statically (i.e. before execution). Finally, it specifies the manner in which the program is executed, i.e. the JSON that is output, or the runtime error if there is one.

The specification is intended to be terse, precise, and illuminate all the subtleties and edge cases in order to allow fully-compatible language reimplementations and tools. The specification employs some standard theoretical computer science techniques, namely [type systems](https://en.wikipedia.org/wiki/Type_system) and [big step operational semantics](https://en.wikipedia.org/wiki/Operational_semantics). If you just want to write Jsonnet code (not build a Jsonnet interpreter or tool), you don't need to read this. You should read the [tutorial](/learning/tutorial.html) and [reference](/ref/language.html) .

Lexing
------

A Jsonnet program is UTF-8 encoded text. The file is a sequence of tokens, separated by optional whitespace and comments. Whitespace consists of space, tab, newline and carriage return. Tokens are lexed greedily. Comments are either single line comments, beginning with a `#` or a `//`, or block comments beginning with `/*` and terminating at the first `*/` encountered within the comment.

*   _id_: Matched by \[\_a-zA-Z\]\[\_a-zA-Z0-9\]\*.

    Some identifiers are reserved as keywords, thus are not in the set _id_: `assert` `else` `error` `false` `for` `function` `if` `import` `importstr` `importbin` `in` `local` `null` `tailstrict` `then` `self` `super` `true`.

*   _number_: As defined by [JSON](https://json.org/) but without the leading minus.

*   _string_: Which can have five quoting forms:

    *   Double-quoted, beginning with `"` and ending with the first subsequent non-quoted `"`
    *   Single-quoted, beginning with `'` and ending with the first subsequent non-quoted `'`
    *   Double-quoted verbatim, beginning with `@"` and ending with the first subsequent non-quoted `"`
    *   Single-quoted verbatim, beginning with `@'` and ending with the first subsequent non-quoted `'`
    *   Text block, beginning with `|||` followed by an optional `-`, then optional whitespace and a new-line. The next non-empty line must be prefixed with some non-zero length whitespace _W_. The block ends at the first subsequent line that is non-empty and does not begin with _W_, and it is an error if this line does not contain some optional whitespace followed by `|||`. The content of the string is the concatenation of all the lines between the two `|||`, which either begin with _W_ (in which case that prefix is stripped) or they are empty lines (in which case they remain as empty lines). The line ending style in the file is preserved in the string. If the beginning `|||` was followed by `-` then the final new-line is stripped from the resulting string. This form cannot be used in `import` statements.

    Double- and single-quoted strings are allowed to span multiple lines, in which case whatever dos/unix end-of-line character your editor inserts will be put in the string. They both understand the following escape characters: `"'\/bfnrt` which have their standard meanings, as well as `\uXXXX` for hexadecimal unicode escapes.

    Verbatim strings eschew all of the normal string escaping, including hexadecimal unicode escapes. Every character in a verbatim string is processed literally, with the exception of doubled end-quotes. Within a verbatim single-quoted string, `''` is processed as `'`, and a verbatim double-quoted string, `""` is processed as `"`.

    In the rest of this specification, the string is assumed to be canonicalized into a sequence of unicode codepoints with no record of the original quoting form as well and any escape characters removed.

*   _symbol_: The following single-character symbols:

    `{}[],.();`

*   _operator_: A sequence of at least one of the following single-character symbols: `!$:~+-&|^=<>*/%`.

    Additionally it is subject to the following rules, which may cause the lexing to terminate with a shorter token:

    *   The sequence `//` is not allowed in an operator.
    *   The sequence `/*` is not allowed in an operator.
    *   The sequence `|||` is not allowed in an operator.
    *   If the sequence has more than one character, it is not allowed to end in any of `+`, `-`, `~`, `!`, `$`.

Abstract Syntax
---------------

The notation used here is as follows: { } denotes zero or more repetitions of a sequence of tokens, and \[ \] represents an optional sequence of tokens. This is not to be confused with `{ }` and `[ ]` which represent tokens in Jsonnet itself.

Note that although the lexer will generate tokens for a wide range of operators, only a finite set are currently parseable, the rest being reserved for possible future use.

_expr ∈ Expr ::= `null` | `true` | `false` | `self` | `$` | _string_ | _number_
| `{` _objinside_ `}`
| `[` \[ _expr_ { `,` _expr_ } \[ `,` \] \] `]`
| `[` _expr_ \[ `,` \] _forspec_ _compspec_ `]`
| _expr_ `.` _id_
| _expr_ `[` \[ _expr_ \] \[ `:` \[ _expr_ \] \[ `:` \[ _expr_ \] \] \] `]`
|`super` `.` _id_
| `super` `[` _expr_ `]`
| _expr_ `(` \[ _args_ \] `)`
| _id_
| `local` _bind_ { `,` _bind_ } `;` _expr_
| `if` _expr_ `then` _expr_ \[ `else` _expr_ \]
| _expr_ _binaryop_ _expr_
| _unaryop_ _expr_
| _expr_ `{` _objinside_ `}`
| `function` `(` \[ _params_ \] `)` _expr_
| _assert_ `;` _expr_
| `import` _string_
| `importstr` _string_
| `importbin` _string_
| `error` _expr_
| _expr_ `in` `super`
_objinside_ ::= { _member_ `,` } \[ _member_ \]
| { _objlocal_ `,` } `[` _expr_ `]` `:` _expr_ \[ { `,` _objlocal_ } \] \[ `,` \] _forspec_ _compspec_
_member_ ::= _objlocal_ | _assert_ | _field_

_field ∈ Field_ ::= _fieldname_ \[ `+` \] _h_ _expr_
| _fieldname_ `(` \[ _params_ \] `)` _h_ _expr_
_h ∈ Hidden_ ::= `:` | `::` | `:::`
_objlocal_ ::= `local` _bind_
_compspec ∈ CompSpec_ ::= { _forspec_ | _ifspec_ }
_forspec_ ::= `for` _id_ `in` _expr_
_ifspec_ ::= `if` _expr_
_fieldname_ ::= _id_ | _string_ | `[` _expr_ `]`
_assert_ ::= `assert` _expr_ \[ `:` _expr_ \]
_bind ∈ Bind_ ::= _id_ `=` _expr_
|_id_ `(` \[ _params_ \] `)` `=` _expr_
_args_ ::= _expr_ { `,` _expr_ } { `,` _id_ `=` _expr_ } \[ `,` \]
| _id_ `=` _expr_ { `,` _id_ `=` _expr_ } \[ `,` \]
_params_ ::= _param_ { `,` _param_ } \[ `,` \]
_param_ ::= _id_ \[ `=` _expr_ \]
_binaryop_::= `*` | `/` | `%` | `+` | `-` | `<<` | `>>` | `<` | `<=` | `>` | `>=` | `==` | `!=` | `in` | `&` | `^` | `|` | `&&` | `||`
_unaryop_ ::= `-` | `+` | `!` | `~`

Associativity and Operator Precedence
-------------------------------------

The abstract syntax by itself cannot unambiguously parse a sequence of tokens. Ambiguities are resolved according to the following rules, which can also be overridden by adding parenthesis symbols `()`.

Everything is left associative. In the case of `assert`, `error`, `function`, `if`, `import`, `importstr`, `importbin`, and `local`, ambiguity is resolved by consuming as many tokens as possible on the right hand side. For example the parentheses are redundant in `local x = 1; (x + x)`. All remaining ambiguities are resolved according to the following decreasing order of precedence:

1.  `e(...)` `e[...]` `e.f`   (application and indexing)
2.  `+` `-` `!` `~`   (the unary operators)
3.  `*` `/` `%`   (these, and the remainder below, are binary operators)
4.  `+` `-`
5.  `<<` `>>`
6.  `<` `>` `<=` `>=` `in`
7.  `==` `!=`
8.  `&`
9.  `^`
10.  `|`
11.  `&&`
12.  `||`

Core Language Subset
--------------------

To make the specification of Jsonnet as simple as possible, many of the language features are represented as syntax sugar. Below is defined the core syntax and the desugaring function from the abstract syntax to the core syntax. Both the static checking rules and the operational semantics are defined at the level of the core language, so it is possible to desugar immediately after parsing.

### Core Syntax

The core language has the following simplifications:

*   The set of identifiers now includes `$`, which is no-longer a special keyword.
*   The following binary operators are removed: `!=` `==` `%` `in`
*   Array slices `[::]` are removed.
*   Array and object comprehensions are replaced with a simple object comprehension construct.
*   Expression-level asserts are removed.
*   Object-level level assert messages are removed.
*   Object-level level assert values are ignored, but their evaluation may still raise an error.
*   Object methods and local functions are removed.
*   Object-level locals are removed.
*   Object field name definitions can only be expressions.
*   The `+:`, `+::`, and `+:::` sugars are removed.
*   Field lookup is only possible through `e[e]`.
*   All conditionals must have an else branch.
*   The keyword `super` can exist on its own.

Commas are no-longer part of this abstract syntax but we may still write them in our notation to make the presentation more clear.

Also removed in the core language are `import`, `importstr`, and `importbin`. The semantics of these constructs is that they are replaced with either the contents of the file, or an error construct if importing failed (e.g. due to I/O errors). In the first case, the file is parsed, desugared, and subject to static checking before it can be substituted. In the case of `importstr`, the file is substituted in the form of a string, so it merely needs to contain valid UTF-8. For `importbin`, the file is substituted as an array of integer numbers between 0 and 255 inclusive.

A given Jsonnet file can be recursively imported via `import`. Thus, the implementation loads files lazily (i.e. during execution) as opposed to via static desugaring. The imported Jsonnet file is parsed and statically checked in isolation. Therefore, the behavior of the import is not affected by the environment into which it is imported. The files are cached by filename, so that even if the file changes on disk during Jsonnet execution, referential transparency is maintained.

_e ∈ Core_ ::= `null` | `true` | `false` | `self` | `super` | _string_ | _number_
| `{` { `assert` _e_ } { `[` _e_ `]` _h_ _e_ } `}`
| `{` `[` _e_ `]` `:` _e_ `for` _id_ `in` _e_ `}`
| `[` { _e_ } `]`
| _e_ `[` _e_ `]`
| _e_ `(` { _e_ } { _id_ `=` _e_ } `)`
| _id_
| `local` _id_ `=` _e_ { _id_ `=` _e_ } `;` _e_
| `if` _e_ `then` _e_ `else` _e_
| _e_ _binaryop_ _e_
| _unaryop_ _e_
| `function` `(` { _id_ `=` _e_ } `)` _e_
| `error` _e_

### Desugaring

Desugaring removes constructs that are not in the core language by replacing them with constructs that are. It is defined via the following functions, which proceed by syntax-directed recursion. If a function is not defined on a construct then it simply recurses into the sub-expressions of that construct. Note that we import the standard library at the top of every file, and some of the desugarings call functions defined in the standard library. Their behavior is specified by implementation. However not all standard library functions are written in Jsonnet. The ones that are built into the interpreter (e.g. reflection) will be given special operational semantics rules with the rest of the core language constructs.

_desugar_: Expr → Core. This desugars a Jsonnet file. Let \\(e\_{std}\\) be the parsed content of [std.jsonnet](https://github.com/google/jsonnet/blob/master/stdlib/std.jsonnet).

\\\[ desugar(e) = desugar\_{expr}(\\local{\\texttt{\\$std} = e\_{std}}{\\local{\\texttt{std} = \\texttt{\\$std}}{e}}, false) \\\]

_desugarexpr_: (Expr × Boolean) → Core: This desugars an expression. The second parameter of the function tracks whether we are within an object.

\\\[ desugar\_{expr}(\\{ \\objlocal{bind\_1} \\ldots \\objlocal{bind\_n}, assert\_1 \\ldots assert\_m, field\_1 \\ldots field\_p \\}, b) = \\\\ \\hspace{10mm} \\textrm{let }binds = \\left\\{\\begin{array}{ll} bind\_1 \\ldots bind\_n & \\textrm{if }b \\\\ bind\_1 \\ldots bind\_n, \\objlocal{$ = \\self} & \\textrm{otherwise} \\\\ \\end{array}\\right. \\\\ \\hspace{10mm} \\textrm{let } obj = \\{ \\\\ \\hspace{20mm} desugar\_{assert}(assert\_1, binds) \\ldots desugar\_{assert}(assert\_m, binds), \\\\ \\hspace{20mm} desugar\_{field}(field\_1, binds) \\ldots desugar\_{field}(field\_p, binds, b) \\\\ \\hspace{10mm} \\} \\\\ \\hspace{10mm} \\textrm{in } \\left\\{\\begin{array}{ll} \\local{\\texttt{\\$outerself} = \\self, \\texttt{\\$outersuper} = \\super} {obj} & \\textrm{if }b \\\\ obj & \\textrm{otherwise} \\\\ \\end{array}\\right. \\\\ \\\]

\\\[ desugar\_{expr}(\\object{ \\objlocal{bind\_1} \\ldots \\objlocal{bind\_m}, \[e\_f\] : e\_{body}, \\objlocal{bind\_m+1} \\ldots \\objlocal{bind\_n} forspec\\ compspec }, b) = \\\\ \\hspace{10mm} \\textrm{Let } arr \\textrm{ fresh and } x\_1 \\ldots x\_n \\textrm{ be the sequence of variables defined in }forspec\\ compspec \\\\ \\hspace{10mm} \\object{ \\\\ \\hspace{20mm} \[desugar\_{expr}(\\local{x\_1=arr\[0\] \\ldots x\_n=arr\[n-1\]}{e\_f}, b)\]: \\\\ \\hspace{30mm} desugar\_{expr}( \\local{x\_1=arr\[0\] \\ldots x\_n=arr\[n-1\]}{ \\local{bind\_1 \\ldots bind\_n}{e\_{body}} }, true) \\\\ \\hspace{20mm} \\textrm{ for } arr \\textrm{ in } desugar\_{expr}(\[ \[x\_1 \\ldots x\_n\] forspec\\ compspec, b): \\\\ \\hspace{10mm}} \\\]

\\\[ desugar\_{expr}(\[e\\ forspec\\ compspec\], b) = desugar\_{arrcomp}(e, forspec\\ compspec, b) \\\]

\\\[ desugar\_{expr}(\\local {bind\_1 \\ldots bind\_n} e, b) = \\local {desugar\_{bind}(bind\_1, b) \\ldots desugar\_{bind}(bind\_n, b)} {desugar\_{expr}(e, b)} \\\]

\\\[ desugar\_{expr}(e \\{ objinside \\}, b) = desugar\_{expr}(e + \\{ objinside \\}, b) \\\]

\\\[ desugar\_{expr}(\\function{p\_1 \\ldots p\_n}e, b) = \\function{desugar\_{param}(p\_1, b)\\ldots desugar\_{param}(p\_n, b)} desugar\_{expr}(e, b) \\\]

\\\[ desugar\_{expr}(\\assert e ; e', b) = desugar\_{expr}(\\assert e : \\texttt{"Assertion failed"} ; e', b) \\\]

\\\[ desugar\_{expr}(\\assert e : e' ; e'', b) = desugar\_{expr}(\\if {e} {e''} {\\error{e'}}, b) \\\]

\\\[ desugar\_{expr}(e\[e':e'':\], b) = desugar\_{expr}(e\[e':e'':\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[e':e''\], b) = desugar\_{expr}(e\[e':e'':\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[e'::e'''\], b) = desugar\_{expr}(e\[e':\\null:e'''\], b) \\\]

\\\[ desugar\_{expr}(e\[:e'':e'''\], b) = desugar\_{expr}(e\[\\null:e'':e'''\], b) \\\]

\\\[ desugar\_{expr}(e\[e':\], b) = desugar\_{expr}(e\[e':\\null:\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[:e'':\], b) = desugar\_{expr}(e\[\\null:e'':\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[:e''\], b) = desugar\_{expr}(e\[\\null:e'':\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[::e'''\], b) = desugar\_{expr}(e\[\\null:\\null:e'''\], b) \\\]

\\\[ desugar\_{expr}(e\[::\], b) = desugar\_{expr}(e\[\\null:\\null:\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[:\], b) = desugar\_{expr}(e\[\\null:\\null:\\null\], b) \\\]

\\\[ desugar\_{expr}(e\[e':e'':e'''\], b) = desugar\_{expr}(\\texttt{\\$std.slice}(e, e', e'', e'''), b) \\\]

\\\[ desugar\_{expr}(\\ifnoelse e e'), b) = \\if {desugar\_{expr}(e, b)} {desugar\_{expr}(e', b)} {\\null} \\\]

\\\[ desugar\_{expr}(e.id, b) = desugar\_{expr}(e, b)\[\\texttt{"}id\\texttt{"}\] \\\]

\\\[ desugar\_{expr}(\\super.id, b) = \\super\[\\texttt{"}id\\texttt{"}\] \\\]

\\\[ desugar\_{expr}(e \\mathop{\\textrm{!=}} e', b) = desugar\_{expr}(!(e \\mathop{==} e'), b) \\\]

\\\[ desugar\_{expr}(e \\mathop{==} e', b) = desugar\_{expr}(\\texttt{\\$std.equals}(e, e'), b) \\\]

\\\[ desugar\_{expr}(e \\mathop{\\%} e', b) = desugar\_{expr}(\\texttt{\\$std.mod}(e, e'), b) \\\]

\\\[ desugar\_{expr}(e \\mathop{\\texttt{in}} e', b) = desugar\_{expr}(\\texttt{\\$std.objectHasEx}(e', e, \\texttt{true}), b) \\\]

\\\[ desugar\_{expr}(e \\mathop{\\texttt{in}} \\texttt{super}, b) = desugar\_{expr}(\\texttt{\\$std.objectHasEx}(\\texttt{super}, e, \\texttt{true}), b) \\\]

_desugarassert_: (Field × \[Bind\]) → Field. This desugars object assertions.

\\\[ desugar\_{assert}(\\assert e, binds) = desugar\_{assert}(\\assert e : \\texttt{"Assertion failed"}, binds) \\\]

\\\[ desugar\_{assert}(\\assert e : e', binds) = \\assert{desugar\_{expr}(\\local{binds}{\\if{e}{\\null}{\\error{e'}}}, \\true)} \\\]

_desugarfield_: (Field × \[Bind\] × Boolean) → Field. This desugars object fields. Recall that _h_ ranges over `:`, `::`, `:::`. The boolean records whether the object containing this field is itself in another object. The notation _string(id)_ means converting the identifier token to a string literal.

\\\[ desugar\_{field}(id \\mathrel{h} e, binds, b) = desugar\_{field}(\[string(id)\] \\mathrel{h} e), binds, b) \\\]

\\\[ desugar\_{field}(id \\mathrel{+h} e, binds, b) = desugar\_{field}(\[string(id)\] \\mathrel{+h} e), binds, b) \\\]

\\\[ desugar\_{field}(id(params) \\mathrel{h} e, binds, b) = desugar\_{field}(\[string(id)\](params) \\mathrel{h} e), binds, b) \\\]

\\\[ desugar\_{field}(string \\mathrel{h} e, binds, b) = desugar\_{field}(\[string\] \\mathrel{h} e), binds, b) \\\]

\\\[ desugar\_{field}(string \\mathrel{+h} e, binds, b) = desugar\_{field}(\[string\] \\mathrel{+h} e), binds, b) \\\]

\\\[ desugar\_{field}(string(params) \\mathrel{h} e, binds, b) = desugar\_{field}(\[string\](params) \\mathrel{h} e), binds, b) \\\]

\\\[ desugar\_{field}(\[e\] \\mathrel{h} e', binds, b) = \[desugar\_{expr}(e, b)\] \\mathrel{h} desugar\_{expr}(\\local{binds}{e}, \\true) \\\]

\\\[ desugar\_{field}(\[e\] \\mathrel{+h} e', binds, b) = \\\\ \\hspace{10mm} \\textrm{let } e'' = e\[\\texttt{\\$outerself} / \\self, \\texttt{\\$outersuper} / \\super\] \\\\ \\hspace{10mm} \\textrm{let } e''' = \\if{e''\\mathrel{\\texttt{in}} \\super}{\\super\[e''\] + {e'}}{e'} \\\\ \\hspace{10mm} \\textrm{in } desugar\_{field}(\[e\] \\mathrel{h} e''', binds, b) \\\]

\\\[ desugar\_{field}(\[e\](params) \\mathrel{h} e', binds, b) = desugar\_{field}(\[e\] \\mathrel{h} \\function{params}{e'}, binds, b) \\\]

_desugarbind_: (Bind × Boolean) → Field. This desugars local bindings.

\\\[ desugar\_{bind}(id \\texttt{=} e, b) = id \\texttt{=} desugar\_{expr}(e, b) \\\]

\\\[ desugar\_{bind}(id(params) \\texttt{=} e, b) = id \\texttt{=} desugar\_{expr}(\\function{params}e, b) \\\]

_desugarparam_: (Param × Boolean) → Param. This desugars function parameters.

\\\[ desugar\_{param}(id, b) = id \\texttt{=} \\error{\\texttt{"Parameter not bound"}} \\\]

\\\[ desugar\_{param}(id \\texttt{=} e, b) = id \\texttt{=} desugar\_{expr}(e, b) \\\]

_desugararrcomp_: (Expr × CompSpec × Boolean) → Field. This desugars array comprehensions.

\\\[ desugar\_{arrcomp}(e, \\textrm{if }e'\\ compspec, b) = desugar\_{expr}(\\if{e'}{desugar\_{arrcomp}(e, compspec, b)}{\[\\ \]}, b) \\\]

\\\[ desugar\_{arrcomp}(e, \\textrm{if }e', b) = desugar\_{expr}(\\if{e'}{\[e\]}{\[\\ \]}, b) \\\]

\\\[ desugar\_{arrcomp}(e, \\textrm{for }x\\textrm{ in }e'\\ compspec, b) = \\\\ \\hspace{10mm}\\textrm{Let }arr, i\\textrm{ fresh} \\\\ \\hspace{10mm}desugar\_{expr}( \\local{arr = e'}{ \\texttt{\\$std.join}(\\\\\\hspace{20mm}\[\\ \], \\texttt{\\$std.makeArray}( \\texttt{\\$std.length}(arr), \\function{i}{\\local{x = arr\[i\]}{desugar\_{arrcomp}(e, compspec, b)}} )) }, b ) \\\]

\\\[ desugar\_{arrcomp}(e, \\textrm{for }x\\textrm{ in }e', b) = \\\\ \\hspace{10mm}\\textrm{Let }arr, i\\textrm{ fresh} \\\\ \\hspace{10mm}desugar\_{expr}( \\local{arr = e'}{ \\texttt{\\$std.join}(\\\\\\hspace{20mm}\[\\ \], \\texttt{\\$std.makeArray}( \\texttt{\\$std.length}(arr), \\function{i}{\\local{x = arr\[i\]}{\[e\]}} )) }, b ) \\\]

### Static Checking

After the Jsonnet program is parsed and desugared, a syntax-directed algorithm is employed to reject programs that contain certain classes of errors. This is presented like a static type system, except that there are no static types. Programs are only rejected if they use undefined variables, or if `self`, `super` or `$` are used outside the bounds of an object. In the core language, `$` has been desugared to a variable, so its checking is implicit in the checking of bound variables.

The static checking is described below as a judgement \\(Γ ⊢ e\\), where \\(Γ\\) is the set of variables in scope of \\(e\\). The set \\(Γ\\) initially contains only `std`, the implicit standard library. In the case of imported files, each jsonnet file is checked independently of the other files.

\\\[ \\rule{chk-lit} { } { \\\_ ⊢ \\null, \\true, \\false, string, number } \\\]

\\\[ \\rule{chk-self} { \\self ∈ Γ } { Γ ⊢ \\self } \\\]

\\\[ \\rule{chk-super} { \\super ∈ Γ } { Γ ⊢ \\super } \\\]

\\\[ \\rule{chk-object} { Γ ⊢ e\_1 \\ldots e\_m \\\\ Γ ∪ \\{\\self,\\super\\} ⊢ e'\_1 \\ldots e'\_n \\\\ ∀ i,j: e\_i ∈ string ∧ e\_j = e\_i ⇒ i = j } { Γ ⊢ \\object{\[e\_1\] h\_1 e'\_1 \\ldots \[e\_m\] h\_m e'\_m, \\assert e'\_{m+1} \\ldots \\assert e'\_n} } \\\]

\\\[ \\rule{chk-object-comp} { Γ ∪ \\{x\\} ⊢ e\_1 \\\\ Γ ∪ \\{x,\\self,\\super\\} ⊢ e\_2 \\\\ Γ ⊢ e\_3 } { Γ ⊢ \\ocomp{e\_1}{e\_2}{x}{e\_3} } \\\]

\\\[ \\rule{chk-array} { Γ ⊢ e\_1 \\ldots e\_n } { Γ ⊢ \\array{e\_1 \\ldots e\_n} } \\\]

\\\[ \\rule{chk-array-index} { Γ ⊢ e \\\\ Γ ⊢ e' } { Γ ⊢ e\[e'\] } \\\]

\\\[ \\rule{chk-apply} { Γ ⊢ e \\\\ ∀ i∈\\{1\\ldots n\\}: Γ ⊢ e\_i \\\\ ∀ i,j∈\\{1\\ldots n\\}: x\_i = x\_j ⇒ i = j } { Γ ⊢ e(x\_1 = e\_1 \\ldots x\_n = e\_n) } \\\]

\\\[ \\rule{chk-var} { x ∈ Γ } { Γ ⊢ x } \\\]

\\\[ \\rule{chk-local} { Γ ∪ \\{x\_1 \\ldots x\_n\\} ⊢ e\_1 \\ldots e\_n, e \\\\ ∀ i,j: x\_i = x\_j ⇒ i = j } { Γ ⊢ \\local{\\assign{x\_1}{e\_1} \\ldots \\assign{x\_n}{e\_n}}e } \\\]

\\\[ \\rule{chk-if} { Γ ⊢ e\_1, e\_2, e\_3 } { Γ ⊢ \\if{e\_1}{e\_2}{e\_3} } \\\]

\\\[ \\rule{chk-binary} { Γ ⊢ e\_L, e\_R } { Γ ⊢ \\binary{e\_L}{sym}{e\_R} } \\\]

\\\[ \\rule{chk-unary} { Γ ⊢ e } { Γ ⊢ \\unary{sym}{e} } \\\]

\\\[ \\rule{chk-function} { ∀ i ∈ \\{m+1 \\ldots n\\}: Γ ∪ \\{x\_1 \\ldots x\_n\\} ⊢ e\_i \\\\ Γ ∪ \\{x\_1 \\ldots x\_n\\} ⊢ e' \\\\ ∀ i,j: x\_i = x\_j ⇒ i = j } { Γ ⊢ \\function{x\_1\\ldots x\_m, x\_{m+1}=e\_{m+1}\\ldots x\_n=e\_n}{e'} } \\\]

\\\[ \\rule{chk-import} { } { Γ ⊢ \\import{s} } \\\]

\\\[ \\rule{chk-importstr} { } { Γ ⊢ \\importstr{s} } \\\]

\\\[ \\rule{chk-importbin} { } { Γ ⊢ \\importbin{s} } \\\]

\\\[ \\rule{chk-error} { Γ ⊢ e } { Γ ⊢ \\error{e} } \\\]

### Operational Semantics

We present two sets of operational semantics rules. The first defines the judgement \\(e ↓ v\\) which represents the execution of Jsonnet expressions into Jsonnet values. The other defines the judgement \\(v ⇓ j\\) which represents manifestation, the process by which Jsonnet values are converted into JSON values.

We model both explicit runtime errors (raised by the error construct) and implicit runtime errors (e.g. array bounds errors) as stuck execution. Errors can occur both in the \\(e ↓ v\\) judgement and in the \\(v ⇓ j\\) judgement (because it is defined in terms of \\(e ↓ v\\)).

#### Jsonnet Values

When executed, Jsonnet expressions yield Jsonnet values. These need to be manifested, an additional step, to get JSON values. The differences between Jsonnet values and JSON values are: 1) Jsonnet values contain functions (which are not representable in JSON). 2) Due to the lazy semantics, both object fields and array elements have yet to be executed to yield values. 3) Object assertions still need to be checked.

Execution of a statically-checked expression will never yield an object with duplicate field names. By abuse of notation, we consider two objects to be equivalent even if their fields and assertions are re-ordered. However this is not true of array elements or function parameters.

_v_ ∈ _Value_ \= _Primitive_ ∪ _Object_ ∪ _Function_ ∪ _Array_
_Primitive_ ::= `null` | `true` | `false` | _string_ | _double_
_o_∈ _Object_ ::= `{` { `assert` _e_ } { _string_ _h_ _e_ } `}`
_Function_ ::= `function (` { _id_\=_e_ } `)` _e_
_a_ ∈ _Array_ ::= `[` { _e_ } `]`

#### Hidden status inheritance

The hidden status of fields is preserved over inheritance if the right hand side uses the `:` form. This is codified with the following function:

\\\[ h\_L + h\_R = \\left\\{\\begin{array}{ll} h\_L & \\textrm{if }h\_R = \\texttt{:} \\\\ h\_R & \\textrm{otherwise} \\\\ \\end{array}\\right. \\\]

#### Capture-Avoiding Substitution

The rules for capture-avoiding variable substitution \[_e_/_id_\] are an extension of those in the [lambda calculus](https://en.wikipedia.org/wiki/Lambda_calculus).

Let y ≠ x.

`self`\[_e_/_x_\] = `self`

`super`\[_e_/_x_\] = `super`

_x_\[_e_/_x_\] = _e_

_y_\[_e_/_x_\] = _y_

`{` ... `assert` _e'_ ... `[`_e''_`]` _h_ _e'''_ ... `}`\[_e_/_x_\] = `{` ... `assert` _e'_\[_e_/_x_\] ... `[`_e'_\[_e_/_x_\]`]` _h_ _e''_\[_e_/_x_\] ... `}`

`{` `[`_e'_`]``:` _e''_ `for` _x_ `in` _e'''_ `}`\[_e_/_x_\] = `{` `[`_e'_`]``:` _e''_ `for` _x_ `in` _e'''_\[_e_/_x_\] `}`

`{` `[`_e'_`]``:` _e''_ `for` _y_ `in` _e'''_ `}`\[_e_/_x_\] = `{` `[`_e'_\[_e_/_x_\]`]:` _e''_\[_e_/_x_\] `for` _y_ `in` _e'''_ \[_e_/_x_\] `}`

(`local` ... _y_`=`_e'_ ... `;` _e''_) \[_e_/_x_\] = `local` ... _y_`=`_e'_ ... `;` _e''_

(If any variable matches.)

(`local` ... _y_`=`_e'_ ... `;` _e''_) \[_e_/_x_\] = `local` ... _y_`=`_e'_\[_e_/_x_\] ... `;`_e''_\[_e_/_x_\]

(If no variable matches.)

(`function` `(` ... _y_\=_e'_ ... `)` _e''_)\[_e_/_x_\] = `function` `(` ... _y_\=_e'_ ... `)`_e''_

(If any param matches.)

(`function` `(` ... _y_\=_e'_ ... `)` _e''_)\[_e_/_x_\] = `function` `(` ... _y_\=_e'_\[_e_/_x_\] ... `)` _e''_\[_e_/_x_\]

(If no param matches.)

Otherwise, _e'_ \[_e_/_x_\] proceeds via syntax-directed recursion into subterms of _e'_.

The rules for keyword substitution ⟦_e_/_kw_⟧ for _kw_ ∈ { `self`, `super` } avoid substituting keywords that are captured by nested objects:

`self` ⟦_e_/`self`⟧ = _e_

`super` ⟦_e_/`super`⟧ = _e_

`self` ⟦_e_/`super`⟧ = `self`

`super` ⟦_e_/`self`⟧ = `super`

`{` ... `assert` _e'_ ... `[`_e''_`]`_h_ _e'''_ ... `}` ⟦_e_/_kw_⟧ = `{` ... `assert` _e'_ ... `[`_e''_⟦_e_/_kw_⟧`]`_h_ _e'''_ ... `}`

`{` `[`_e'_`]``:` _e''_ `for` _x_ `in` _e'''_ `}`⟦_e_/_kw_⟧ = `{` `[`_e'_⟦_e'_/_kw_⟧`]``:` _e''_ `for` _x_ `in` _e'''_⟦_e_/_kw_⟧ `}`

Otherwise, _e'_⟦_e'_/_kw_⟧ proceeds via syntax-directed recursion into the subterms of _e'_.

#### Execution

The following big step operational semantics rules define the execution of Jsonnet programs, i.e. the reduction of a Jsonnet program _e_ into its Jsonnet value _v_ via the judgement \\(e ↓ v\\).

Let _f_ range over strings, as used in object field names.

\\\[ \\rule{value} { v ∈ \\{\\null, \\true, \\false\\} ∪ String ∪ Number ∪ Function ∪ Array } { v ↓ v } \\\]

\\\[ \\rule{object} { ∀i∈\\{1\\ldots p\\}: e\_i ↓ f\_i \\\\ ∀i∈\\{p+1 \\ldots n\\}: e\_i ↓ \\null \\\\ ∀i,j∈\\{1\\ldots p\\}: f\_i = f\_j ⇒ i = j \\\\ o = \\object{ \\assert{e\_1} \\ldots \\assert{e\_m}, f\_1\\mathop{h\_1}e''\_1 \\ldots f\_p\\mathop{h\_p}e''\_p } } { \\object{\\assert{e\_1} \\ldots \\assert{e\_m}, \[e'\_1\]\\mathop{h\_1}e''\_1 \\ldots \[e'\_n\]\\mathop{h\_n}e''\_n } ↓ o } \\\]

\\\[ \\rule{object-comp} { e\_{arr} ↓ \[ e\_1 \\ldots e\_n \] \\\\ ∀i∈\\{1 \\ldots n\\}: e\_{field}\[e\_i/x\] ↓ f\_i \\\\ ∀i,j∈\\{1\\ldots n\\}: f\_i = f\_j ≠ \\null ⇒ i = j \\\\ \\{ (f'\_1, e'\_1) \\ldots (f'\_m, e'\_m) \\} = \\{ (f\_i, e\_{body}\[e\_i/x\]) \\ | \\ i ∈\\{1\\ldots n\\} ∧ f\_i ≠ \\null \\} \\\\ o = \\object{f'\_1: e'\_1 \\ldots f'\_m: e'\_m} } { \\ocomp{e\_{field}}{e\_{body}}{x}{e\_{arr}} ↓ o } \\\]

\\\[ \\rule{array-index} { e ↓ \\array{e\_0 \\ldots e\_n} \\\\ e' ↓ i ∈ \\{ 0 \\ldots n \\} \\\\ e\_i ↓ v } { \\index{e}{e'} ↓ v } \\\]

\\\[ \\rule{object-index} { e ↓ o = \\object{\\assert{e'''\_1} \\ldots \\assert{e'''\_m}, f\_1 h\_1 e''\_1 \\ldots f\_n h\_n e''\_n} \\\\ ∀j ∈ \\{1 \\ldots m \\}: e'''\_j⟦ o / \\self, \\{\\} / \\super ⟧↓\\\_ \\\\ e' ↓ f\_i \\\\ e''\_i ⟦ o / \\self, \\{\\} / \\super ⟧ ↓ v } { \\index{e}{e'} ↓ v } \\\]

\\\[ \\rule{apply} { e\_0 ↓ \\function{y\_1=e'\_1 \\ldots y\_p=e'\_p}{e\_b} \\\\ ∀i∈\\{m+1 \\ldots n\\}: x\_i ∈ \\{y\_1 \\ldots y\_p\\} \\\\ ∀i∈\\{1 \\ldots p\\}: e''\_i = \\left\\{\\begin{array}{ll} e\_i & \\textrm{if } i ≤ m \\\\ e\_j & \\textrm{if } y\_i=x\_j \\textrm{ for some } j \\\\ e'\_i & \\textrm{otherwise}\\\\ \\end{array}\\right. \\\\ (\\local{y\_1=e''\_1 \\ldots y\_p=e''\_p}{e\_b}) ↓ v } { e\_0(e\_1 \\ldots e\_m, x\_{m+1}=e\_{m+1} \\ldots x\_n = e\_n) ↓ v } \\\]

\\\[ \\rule{local} { e ↓ v } { \\local{\\\_}e ↓ v } \\\]

\\\[ \\rule{local-var} { binds = \\assign{x\_1}{e\_1} \\ldots \\assign{x\_n}{e\_n} \\\\ \\local{binds}{e\[\\local{binds}{e\_1} / x\_1 \\ldots \\local{binds}{e\_n} / x\_n \]} ↓ v } { \\local{binds}e ↓ v } \\\]

\\\[ \\rule{if-true} { e\_1 ↓ \\true \\hspace{15pt} e\_2 ↓ v } { \\if{e\_1}{e\_2}{e\_3} ↓ v } \\\]

\\\[ \\rule{if-false} { e\_1 ↓ \\false \\hspace{15pt} e\_3 ↓ v } { \\if{e\_1}{e\_2}{e\_3} ↓ v } \\\]

\\\[ \\rule{object-inherit} { e^L ↓ \\object{ \\assert e''^L\_1 \\ldots \\assert e''^L\_n,\\ f\_1 h^L\_1 e^L\_1 \\ldots f\_m h^L\_m e^L\_m,\\ f'^L\_1 h'^L\_1 e'^L\_1 \\ldots f'^L\_p h'^L\_p e'^L\_p } \\\\ e^R ↓ \\object{ \\assert e''^R\_1 \\ldots \\assert e''^R\_q,\\ f\_1 h^R\_1 e^R\_1 \\ldots f\_m h^R\_m e^R\_m,\\ f'^R\_1 h'^R\_1 e'^R\_1 \\ldots f'^R\_r h'^R\_r e'^R\_r } \\\\ \\{ f'^L\_1 \\ldots f'^L\_p \\} ∩ \\{ f'^R\_1 \\ldots f'^R\_r \\} = ∅ \\\\ x, y \\textrm{ fresh} \\hspace{15pt} \\textrm{let } S = λe . e⟦x/\\self, y/\\super⟧ \\\\ e\_s = \\super + \\object{ \\assert S(e''^L\_1) \\ldots \\assert S(e''^L\_n),\\\\ \\hspace{20mm} f\_1 h^L\_1 S(e^L\_1) \\ldots f\_m h^L\_m S(e^L\_m),\\ f'^L\_1 h'^L\_1 S(e'^L\_1) \\ldots f'^L\_p h'^L\_p S(e'^L\_p) } \\\\ ∀i∈\\{1 \\ldots m\\}: h'''\_i = h^L\_i + h^R\_i ∧ e'''\_i = (\\local{x = \\self, y = \\super}{e^R\_i ⟦e\_s / \\super⟧}) \\\\ o = \\{ \\\\ \\hspace{5mm} \\assert e''^L\_1 \\ldots \\assert e''^L\_n, \\ \\assert e''^R\_1 \\ldots \\assert e''^R\_q, \\\\ \\hspace{5mm} f'^L\_1 h'^L\_1 e'^L\_1 \\ldots f'^L\_p h'^L\_p e'^L\_p, \\ f'^R\_1 h'^R\_1 e'^R\_1 \\ldots f'^R\_r h'^R\_r e'^R\_r, \\ f\_1 h'''\_1 e'''\_m \\ldots f\_m h'''\_m e'''\_m \\\\ \\} } { e^L \\texttt{ + } e^R ↓ o } \\\]

\\\[ \\rule{array-concat} { e ↓ \\array{e\_0 \\ldots e\_m} \\\\ e' ↓ \\array{e\_{m+1} \\ldots e\_n} } { e + e' ↓ \\array{e\_1 \\ldots e\_n} } \\\]

\\\[ \\rule{string-concat} { e\_L ↓ v\_L \\hspace{15pt} e\_R ↓ v\_R \\\\ v\_L ∈ String \\vee v\_R ∈ String } { e\_L \\texttt{ + } e\_R ↓ stringconcat(tostring(v\_L), tostring(v\_R)) } \\\]

\\\[ \\rule{less-than-true} { \\texttt{std.cmp}(e\_L, e\_R) ↓ -1 } { e\_L < e\_R ↓ \\texttt{true} } \\\]

\\\[ \\rule{less-than-false} { \\texttt{std.cmp}(e\_L, e\_R) ↓ r \\\\ r ∈ \\{0, 1\\} } { e\_L < e\_R ↓ \\texttt{false} } \\\]

\\\[ \\rule{greater-than-true} { \\texttt{std.cmp}(e\_L, e\_R) ↓ 1 } { e\_L < e\_R ↓ \\texttt{true} } \\\]

\\\[ \\rule{greater-than-false} { \\texttt{std.cmp}(e\_L, e\_R) ↓ r \\\\ r ∈ \\{-1, 0\\} } { e\_L < e\_R ↓ \\texttt{false} } \\\]

\\\[ \\rule{less-or-equal-true} { \\texttt{std.cmp}(e\_L, e\_R) ↓ r \\\\ r ∈ \\{-1, 0\\} } { e\_L <= e\_R ↓ \\texttt{true} } \\\]

\\\[ \\rule{less-or-equal-false} { \\texttt{std.cmp}(e\_L, e\_R) ↓ 1 } { e\_L <= e\_R ↓ \\texttt{false} } \\\]

\\\[ \\rule{greater-or-equal-true} { \\texttt{std.cmp}(e\_L, e\_R) ↓ r \\\\ r ∈ \\{0, 1\\} } { e\_L >= e\_R ↓ \\texttt{true} } \\\]

\\\[ \\rule{greater-or-equal-false} { \\texttt{std.cmp}(e\_L, e\_R) ↓ -1 } { e\_L >= e\_R ↓ \\texttt{false} } \\\]

\\\[ \\rule{cmp-array-left-empty} { e\_L ↓ \\array{} \\hspace{15pt} e\_R ↓ \\array{a\_0 \\ldots a\_{n - 1}} \\\\ n > 0 } { \\texttt{std.cmp}(e\_L, e\_R) ↓ -1 } \\\]

\\\[ \\rule{cmp-array-right-empty} { e\_L ↓ \\array{a\_0 \\ldots a\_{n - 1}} \\hspace{15pt} e\_R ↓ \\array{} \\\\ n > 0 } { \\texttt{std.cmp}(e\_L, e\_R) ↓ 1 } \\\]

\\\[ \\rule{cmp-array-both-empty} { e\_L ↓ \\array{} \\hspace{15pt} e\_R ↓ \\array{} } { \\texttt{std.cmp}(e\_L, e\_R) ↓ 0 } \\\]

\\\[ \\rule{cmp-array-first-different} { e\_L ↓ \\array{a\_0 \\ldots a\_{n - 1}} \\hspace{15pt} e\_R ↓ \\array{b\_0 \\ldots b\_{m - 1}} \\\\ n > 0 \\hspace{15pt} m > 0 \\\\ \\texttt{std.cmp}(a\_0, b\_0) ↓ r \\\\ r ≠ 0 } { \\texttt{std.cmp}(e\_L, e\_R) ↓ r } \\\]

\\\[ \\rule{cmp-array-first-equal} { e\_L ↓ \\array{a\_0 \\ldots a\_{n - 1}} \\hspace{15pt} e\_R ↓ \\array{b\_0 \\ldots b\_{m - 1}} \\\\ n > 0 \\hspace{15pt} m > 0 \\\\ \\texttt{std.cmp}(a\_0, b\_0) ↓ 0 \\\\ \\texttt{std.cmp}(\\array{a\_1 \\ldots a\_{n - 1}}, \\array{b\_1 \\ldots b\_{m - 1}}) ↓ r' } { \\texttt{std.cmp}(e\_L, e\_R) ↓ r' } \\\]

\\\[ \\rule{cmp-string} { e\_L ↓ v\_L \\hspace{15pt} e\_R ↓ v\_R \\\\ v\_L ∈ String \\\\ v\_R ∈ String } { \\texttt{std.cmp}(e\_L, e\_R) ↓ stringcmp(v\_L, v\_R) } \\\]

\\\[ \\rule{cmp-number} { e\_L ↓ v\_L \\hspace{15pt} e\_R ↓ v\_R \\\\ v\_L ∈ Number \\\\ v\_R ∈ Number } { \\texttt{std.cmp}(e\_L, e\_R) ↓ numbercmp(v\_L, v\_R) } \\\]

\\\[ \\rule{boolean-and-shortcut} { e\_L ↓ \\false } { e\_L \\texttt{ && } e\_R ↓ \\false } \\\]

\\\[ \\rule{boolean-and-longcut} { e\_L ↓ \\true \\\\ e\_R ↓ b } { e\_L \\texttt{ && } e\_R ↓ b } \\\]

\\\[ \\rule{boolean-or-shortcut} { e\_L ↓ \\true } { e\_L \\texttt{ || } e\_R ↓ \\true } \\\]

\\\[ \\rule{boolean-or-longcut} { e\_L ↓ \\false \\\\ e\_R ↓ b } { e\_L \\texttt{ || } e\_R ↓ b } \\\]

\\\[ \\rule{not-true} { e ↓ \\true } { \\texttt{!} e ↓ \\false } \\\]

\\\[ \\rule{not-false} { e ↓ \\false } { \\texttt{!} e ↓ \\true } \\\]

\\\[ \\rule{primitiveEquals} { e ↓ v \\\\ e' ↓ v' \\\\ b = (v ∈ String ∨ v ∈ Boolean ∨ v ∈ Number ∨ v = \\null) ∧ v = v' } { \\texttt{std.primitiveEquals}(e, e') ↓ b } \\\]

\\\[ \\rule{length-array} { e ↓ \\array{e\_0 \\ldots e\_{n - 1}} } { \\texttt{std.length}(e) ↓ n } \\\]

\\\[ \\rule{length-object} { \\texttt{std.length}(\\texttt{std.objectFieldsEx}(e, false) ↓ n } { \\texttt{std.length}(e) ↓ n } \\\]

\\\[ \\rule{length-string} { e ↓ v ∈ String } { \\texttt{std.length}(e) ↓ strlen(v) } \\\]

\\\[ \\rule{makeArray} { e ↓ n \\\\ e' ↓ f ∈ Function } { \\texttt{std.makeArray}(e, e') ↓ \\array{f(0) \\ldots f(n - 1)} } \\\]

\\\[ \\rule{filter} { e ↓ f ∈ Function \\\\ e' ↓ \\array{e\_0 \\ldots e\_{n - 1}} \\\\ j\_1 \\ldots j\_m = \\{ i \\ |\\ f(e\_i) ↓ \\true \\} } { \\texttt{std.filter}(e, e') ↓ \\array{e\_{j\_1} \\ldots e\_{j\_m}} } \\\]

\\\[ \\rule{type-null} { e ↓ \\texttt{null} } { \\texttt{std.type}(e) ↓ \\texttt{"null"} } \\\]

\\\[ \\rule{type-boolean} { e ↓ v ∈ Boolean } { \\texttt{std.type}(e) ↓ \\texttt{"boolean"} } \\\]

\\\[ \\rule{type-number} { e ↓ v ∈ Number } { \\texttt{std.type}(e) ↓ \\texttt{"number"} } \\\]

\\\[ \\rule{type-string} { e ↓ v ∈ String } { \\texttt{std.type}(e) ↓ \\texttt{"string"} } \\\]

\\\[ \\rule{type-object} { e ↓ v ∈ Object } { \\texttt{std.type}(e) ↓ \\texttt{"object"} } \\\]

\\\[ \\rule{type-function} { e ↓ v ∈ Function } { \\texttt{std.type}(e) ↓ \\texttt{"function"} } \\\]

\\\[ \\rule{type-array} { e ↓ v ∈ Array } { \\texttt{std.type}(e) ↓ \\texttt{"array"} } \\\]

\\\[ \\rule{object-has-ex} { e' ↓ f \\\\ e'' ↓ b' \\\\ e ↓ \\object{\\assert{e'\_1} \\ldots \\assert{e'\_m}, f\_1 h\_1 e\_1 \\ldots f\_n h\_n e\_n } \\\\ b = ∃i: f = f\_i ∧ (h\_i \\mathop{≠} :: \\mathop{∨} b') } { \\texttt{std.objectHasEx}(e, e', e'') ↓ b } \\\]

\\\[ \\rule{object-fields-ex} { e' ↓ b' \\\\ e ↓ \\object{\\assert{e'\_1} \\ldots \\assert{e'\_m}, f\_1 h\_1 e\_1 \\ldots f\_n h\_n e\_n } \\\\ \\{ f'\_1 \\ldots f'\_p \\} = \\{ f\\ |\\ ∃i: f = f\_i ∧ (h\_i \\mathop{≠} :: \\mathop{∨} b') \\} \\\\ ∀i,j∈\\{1 \\ldots p\\}: i≤j ⇒ f'\_i≤f'\_j } { \\texttt{std.objectFieldsEx}(e, e') ↓ \\array{f'\_1 \\ldots f'\_p} } \\\]

String concatenation will implicitly convert one of the values to a string if necessary. This is similar to Java. The referred function \\(tostring\\) returns its argument unchanged if it is a string. Otherwise it will manifest its argument as a JSON value \\(j\\) and unparse it as a single line of text. The referred function \\(strlen\\) returns the number of unicode characters in the string.

The numeric semantics are as follows:

*   **Arithmetic:** Binary `*`, `/`, `+`, `-`, `<`, `<=`, `>`, `>=`, and unary `+` and `-` operate on numbers and have IEEE double precision floating point semantics, except that the special states NaN, Infinity raise errors. Note that `+` is also overloaded on objects, arrays, and when either argument is a string. Also, `<`, `<=`, `>`, `>=` are overloaded on strings and on arrays. In both cases the comparison is performed lexicographically (in case of strings, by unicode codepoint).
*   **Bitwise:** Operators `<<`, `>>`, `&`, `^`, `|` and `~` first convert their operands to signed 64 bit integers, then perform the operations in a standard way, then convert back to IEEE double precision floating point. In shift operations `<<`, `>>`, the right hand value modulo 64 is interpreted as the shift count. Shifting with a negative shift count raises an error.
*   **Standard functions**: The following functions have standard mathematical behavior and operate on IEEE double precision floating point: `std.pow(a, b)`, `std.floor(x)`, `std.ceil(x)`, `std.sqrt(x)`, `std.sin(x)`, `std.cos(x)`, `std.tan(x)`, `std.asin(x)`, `std.acos(x)`, `std.atan(x)`, `std.atan2(y, x)`, `std.log(x)`, `std.log2(x)`, `std.log10(x)`, `std.exp(x)`, `std.mantissa(x)`, `std.exponent(x)` and `std.modulo(a, b)`. Also, `std.codepoint(x)` take a single character string, returning the unicode codepoint as a number, and `std.char(x)` is its inverse.

The `error` operator has no rule because we model errors (both from the language and user-defined) as stuck execution. The semantics of `error` are that its subterm is evaluated to a Jsonnet value. If this is a string, then that is the error that is raised. Otherwise, it is converted to a string using \\(tostring\\) like during string concatenation. The specification does not specify how the error is presented to the user, and whether or not there is a stack trace. Error messages are meant for human inspection, and there is therefore no need to standardize them.

Finally, the function `std.native(x)` takes a string and returns a function configured by the user in a custom execution environment, thus its semantics cannot be formally described here. The function `std.extVar(x)` also takes a string and returns the value bound to that external variable at the time the Jsonnet environment was created.

#### JSON Values

After execution, the resulting Jsonnet value is manifested into a JSON value whose serialized form is the ultimate output. The Manifestation process removes hidden fields, checks assertions, and forces array elements and non-hidden object fields. Attempting to manifest a function raises an error since they do not exist in JSON. JSON values are formalized below.

By abuse of notation, we consider two objects to be equivalent even if their fields are re-ordered. However this is not true of array elements whose ordering is strict.

_j_ ∈ _JValue_ \= _Primitive_ ∪ _JObject_ ∪ _JArray_
_Primitive_ ::= `null` | `true` | `false` | _string_ | _double_
_o_ ∈ _JObject_ ::= `{` { _string_ : _j_ } `}`
_a_ ∈ _Array_ ::= `[` { _j_ } `]`

Note that _JValue_ ⊂ _Value_.

#### Manifestation

Manifestation is the conversion of a Jsonnet value into a JSON value. It is represented with the judgement \\(v⇓j\\). The process requires executing arbitrary Jsonnet code fragments, so the two semantic judgements represented by \\(↓\\) and \\(⇓\\) are mutually recursive. Hidden fields are ignored during manifestation. Functions cannot be manifested, so an error is raised in that case (formalized as stuck execution).

\\\[ \\rule{manifest-value} { j ∈ (\\{\\null, \\true, \\false\\} ∪ String ∪ Number) } { j ⇓ j } \\\]

\\\[ \\rule{manifest-object} { o = \\object{ \\assert e''\_1 \\ldots \\assert e''\_m,\\ f\_1 h\_1 e\_1 \\ldots f\_n h\_n e\_n,\\ f'\_1 :: e'\_1 \\ldots f'\_p :: e'\_p,\\ } \\\\ ∀i∈\\{1\\ldots m\\}: e''\_i⟦o/\\self,\\{\\}/\\super⟧↓\\\_ \\\\ ∀i∈\\{1\\ldots n\\}: e\_i⟦o/\\self,\\{\\}/\\super⟧↓v\_i⇓j\_i\\ ∧\\ h\_i ≠ :: } { o ⇓ \\object{f\_1 : j\_1 \\ldots f\_n : j\_n} } \\\]

\\\[ \\rule{manifest-array} { ∀i∈\\{1\\ldots n\\}: e\_i↓v\_i⇓j\_i } { \\array{e\_1 \\ldots e\_n} ⇓ \\array{j\_1 \\ldots j\_n} } \\\]
