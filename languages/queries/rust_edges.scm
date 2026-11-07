; Rust edge extraction queries
; Captures: @call.name @call.qualifier @call.receiver
;           @import.path @import.name @import.alias @import.module
;           @binding.name @binding.type @binding.call

; ── Calls ───────────────────────────────────────────────────────────────

; f()
(call_expression function: (identifier) @call.name)
; obj.method()
(call_expression function: (field_expression value: (_) @call.receiver field: (field_identifier) @call.name))
; Type::f() / module::f()
(call_expression function: (scoped_identifier path: (_) @call.qualifier name: (identifier) @call.name))
; Generic forms: f::<T>(), Type::f::<T>(), obj.method::<T>()
(call_expression function: (generic_function function: (identifier) @call.name))
(call_expression function: (generic_function function: (scoped_identifier path: (_) @call.qualifier name: (identifier) @call.name)))
(call_expression function: (generic_function function: (field_expression value: (_) @call.receiver field: (field_identifier) @call.name)))

; Macro invocations only resolve to macro definitions
((macro_invocation macro: (identifier) @call.name) (#set! target "macro"))
((macro_invocation macro: (scoped_identifier path: (_) @call.qualifier name: (identifier) @call.name)) (#set! target "macro"))

; Calls inside macro arguments (format!, assert_eq!, vec!, ...) are token
; trees, not expressions: match the token shapes `f(..)`, `x.f(..)`, `T::f(..)`.
; (only `(..)` argument lists: `Type { .. }` is a struct literal, `x[..]` indexing)
((token_tree (identifier) @call.name . (token_tree) @_args) (#match? @_args "^[(]"))
((token_tree (identifier) @call.receiver . "." . (identifier) @call.name . (token_tree) @_args) (#match? @_args "^[(]"))
((token_tree (identifier) @call.qualifier . "::" . (identifier) @call.name . (token_tree) @_args) (#match? @_args "^[(]"))

; ── Imports ─────────────────────────────────────────────────────────────

; use a::b::Item;
(use_declaration argument: (scoped_identifier path: (_) @import.path name: (identifier) @import.name))
; use a::b::Item as Alias;
(use_declaration argument: (use_as_clause path: (scoped_identifier path: (_) @import.path name: (identifier) @import.name) alias: (identifier) @import.alias))
; use a::b::{Item, Other};
(use_declaration argument: (scoped_use_list path: (_) @import.path list: (use_list (identifier) @import.name)))
; use a::b::{Item as Alias};
(use_declaration argument: (scoped_use_list path: (_) @import.path list: (use_list (use_as_clause path: (identifier) @import.name alias: (identifier) @import.alias))))
; use a::b::{c::Item};
(use_declaration argument: (scoped_use_list path: (_) @import.path list: (use_list (scoped_identifier path: (_) name: (identifier) @import.name))))
; use module;
(use_declaration argument: (identifier) @import.module)

; ── Local variable types ────────────────────────────────────────────────

; let x: Type = ...;
(let_declaration pattern: (identifier) @binding.name type: (_) @binding.type)
; let x = Type::new(..); let x = make(..);
(let_declaration pattern: (identifier) @binding.name value: (call_expression function: (_) @binding.call))
; let x = make(..)?;
(let_declaration pattern: (identifier) @binding.name value: (try_expression (call_expression function: (_) @binding.call)))
; let x = Type { .. };
(let_declaration pattern: (identifier) @binding.name value: (struct_expression name: (_) @binding.type))
; fn f(x: Type)
(parameter pattern: (identifier) @binding.name type: (_) @binding.type)

; ── Supertypes ──────────────────────────────────────────────────────────

; impl Trait for Type
((impl_item trait: [
  (type_identifier) @call.name
  (generic_type type: (type_identifier) @call.name)
  (scoped_type_identifier path: (_) @call.qualifier name: (type_identifier) @call.name)
]) (#set! edge "implements"))
; trait A: B + C
((trait_item bounds: (trait_bounds (type_identifier) @call.name)) (#set! edge "inherits"))

; ── Type references not covered by signatures ───────────────────────────

; Type { .. } struct literals
((struct_expression name: (type_identifier) @call.name) (#set! edge "references"))
((struct_expression name: (scoped_type_identifier path: (_) @call.qualifier name: (type_identifier) @call.name)) (#set! edge "references"))

; self.method(..) inside macro arguments
((token_tree (self) @call.receiver . "." . (identifier) @call.name . (token_tree) @_args) (#match? @_args "^[(]"))

; ── More imports and locals ─────────────────────────────────────────────

; use a::b::*;
(use_declaration argument: (use_wildcard (_) @import.glob))
; for x in xs
(for_expression pattern: (identifier) @binding.name value: (_) @binding.iter)
; use a::{b::{self, ..}}  (binds module `b`)
(use_declaration argument: (scoped_use_list path: (_) @import.path list: (use_list (scoped_use_list path: (identifier) @import.name list: (use_list (self))))))

; A function passed as an argument: `.map(checksum)`.
((call_expression arguments: (arguments (identifier) @call.name)) (#set! edge "callback"))
