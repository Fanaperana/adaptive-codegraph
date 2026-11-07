; Python edge extraction queries

; ── Calls ───────────────────────────────────────────────────────────────

; f()  /  Class()
(call function: (identifier) @call.name)
; obj.method()
(call function: (attribute object: (_) @call.receiver attribute: (identifier) @call.name))

; ── Imports ─────────────────────────────────────────────────────────────

; from x import a
(import_from_statement module_name: (_) @import.path name: (dotted_name) @import.name)
; from x import a as b
(import_from_statement module_name: (_) @import.path name: (aliased_import name: (dotted_name) @import.name alias: (identifier) @import.alias))
; import x.y
(import_statement name: (dotted_name) @import.module)
; import x.y as z
(import_statement name: (aliased_import name: (dotted_name) @import.module alias: (identifier) @import.alias))

; ── Local variable types ────────────────────────────────────────────────

; x: Type = ...
(assignment left: (identifier) @binding.name type: (_) @binding.type)
; x = Class(...) / x = make(...)
(assignment left: (identifier) @binding.name right: (call function: (_) @binding.call))
; x = await make(...)
(assignment left: (identifier) @binding.name right: (await (call function: (_) @binding.call)))
; def f(x: Type), def f(x: Type = ...)
(typed_parameter (identifier) @binding.name type: (_) @binding.type)
(typed_default_parameter name: (identifier) @binding.name type: (_) @binding.type)
; with open(...) as f:
(with_item value: (as_pattern (call function: (_) @binding.call) alias: (as_pattern_target (identifier) @binding.name)))
