; TSX edge extraction queries: TypeScript edges plus JSX component usage

; ── Calls ───────────────────────────────────────────────────────────────

(call_expression function: (identifier) @call.name)
(call_expression function: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))
(new_expression constructor: (identifier) @call.name)
(new_expression constructor: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))

; <Component ... /> renders a component (lowercase tags are HTML elements
; and never resolve to project symbols with a capitalized name)
((jsx_opening_element name: (identifier) @call.name) (#match? @call.name "^[A-Z]") (#set! edge "renders"))
((jsx_self_closing_element name: (identifier) @call.name) (#match? @call.name "^[A-Z]") (#set! edge "renders"))
((jsx_opening_element name: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name)) (#set! edge "renders"))
((jsx_self_closing_element name: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name)) (#set! edge "renders"))

; ── Imports ─────────────────────────────────────────────────────────────

(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name !alias))) source: (string) @import.path)
(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name alias: (identifier) @import.alias))) source: (string) @import.path)
(import_statement (import_clause (identifier) @import.default) source: (string) @import.path)
(import_statement (import_clause (namespace_import (identifier) @import.alias)) source: (string) @import.path)

; ── Local variable types ────────────────────────────────────────────────

(variable_declarator name: (identifier) @binding.name type: (type_annotation (_) @binding.type))
(variable_declarator name: (identifier) @binding.name value: (new_expression constructor: (_) @binding.type))
(variable_declarator name: (identifier) @binding.name value: (call_expression function: (_) @binding.call))
(variable_declarator name: (identifier) @binding.name value: (await_expression (call_expression function: (_) @binding.call)))
(required_parameter pattern: (identifier) @binding.name type: (type_annotation (_) @binding.type))
(optional_parameter pattern: (identifier) @binding.name type: (type_annotation (_) @binding.type))
