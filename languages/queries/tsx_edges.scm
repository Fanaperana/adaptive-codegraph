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

; ── Supertypes ──────────────────────────────────────────────────────────

((class_heritage (extends_clause value: (identifier) @call.name)) (#set! edge "inherits"))
((class_heritage (extends_clause value: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))) (#set! edge "inherits"))
((class_heritage (implements_clause (type_identifier) @call.name)) (#set! edge "implements"))
((class_heritage (implements_clause (generic_type name: (type_identifier) @call.name))) (#set! edge "implements"))
((extends_type_clause type: (type_identifier) @call.name) (#set! edge "inherits"))
((extends_type_clause type: (generic_type name: (type_identifier) @call.name)) (#set! edge "inherits"))

; ── Re-exports ──────────────────────────────────────────────────────────

(export_statement (export_clause (export_specifier name: (identifier) @import.name !alias)) source: (string) @import.path)
(export_statement (export_clause (export_specifier name: (identifier) @import.name alias: (identifier) @import.alias)) source: (string) @import.path)
(export_statement "*" source: (string) @import.glob)

; ── Fields and loop variables ───────────────────────────────────────────

; class fields: x = make() / x = new T()
(public_field_definition name: (property_identifier) @binding.name value: (call_expression function: (_) @binding.call))
(public_field_definition name: (property_identifier) @binding.name value: (new_expression constructor: (_) @binding.type))
; this.x = new T() / this.x = make()
(assignment_expression left: (member_expression object: (this) property: (property_identifier)) @binding.name right: (new_expression constructor: (_) @binding.type))
(assignment_expression left: (member_expression object: (this) property: (property_identifier)) @binding.name right: (call_expression function: (_) @binding.call))
; for (const x of xs)
(for_in_statement left: (identifier) @binding.name right: (_) @binding.iter)

; A function passed as an argument: `items.map(parse)`.
((call_expression arguments: (arguments (identifier) @call.name)) (#set! edge "callback"))
