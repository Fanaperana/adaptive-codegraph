; Go edge extraction queries

; ── Calls ───────────────────────────────────────────────────────────────

; f()
(call_expression function: (identifier) @call.name)
; obj.Method() / pkg.Func()
(call_expression function: (selector_expression operand: (_) @call.receiver field: (field_identifier) @call.name))

; ── Imports ─────────────────────────────────────────────────────────────

; import "a/b/pkg"   (bound to the last path segment)
(import_spec !name path: (interpreted_string_literal) @import.module)
; import alias "a/b/pkg"
(import_spec name: (package_identifier) @import.alias path: (interpreted_string_literal) @import.module)

; ── Local variable types ────────────────────────────────────────────────

; x := pkg.New() / x := New()
(short_var_declaration left: (expression_list . (identifier) @binding.name) right: (expression_list . (call_expression function: (_) @binding.call)))
; x := Type{} / x := &Type{}
(short_var_declaration left: (expression_list . (identifier) @binding.name) right: (expression_list . (composite_literal type: (_) @binding.type)))
(short_var_declaration left: (expression_list . (identifier) @binding.name) right: (expression_list . (unary_expression operand: (composite_literal type: (_) @binding.type))))
; var x Type
(var_spec name: (identifier) @binding.name type: (_) @binding.type)
; func f(x Type) and method receivers
(parameter_declaration name: (identifier) @binding.name type: (_) @binding.type)
