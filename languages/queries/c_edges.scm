; C edge extraction queries

; ── Calls ───────────────────────────────────────────────────────────────

(call_expression function: (identifier) @call.name)
; s->fn() / s.fn()  (function pointers in structs)
(call_expression function: (field_expression argument: (_) @call.receiver field: (field_identifier) @call.name))

; ── Includes ────────────────────────────────────────────────────────────

(preproc_include path: (string_literal) @import.module)
(preproc_include path: (system_lib_string) @import.module)

; ── Local variable types ────────────────────────────────────────────────

(declaration type: (_) @binding.type declarator: (init_declarator declarator: (identifier) @binding.name))
(declaration type: (_) @binding.type declarator: (init_declarator declarator: (pointer_declarator declarator: (identifier) @binding.name)))
(parameter_declaration type: (_) @binding.type declarator: (identifier) @binding.name)
(parameter_declaration type: (_) @binding.type declarator: (pointer_declarator declarator: (identifier) @binding.name))
