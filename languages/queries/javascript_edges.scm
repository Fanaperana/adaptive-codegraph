; JavaScript edge extraction queries (JSX included)

; ── Calls ───────────────────────────────────────────────────────────────

(call_expression function: (identifier) @call.name)
(call_expression function: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))
(new_expression constructor: (identifier) @call.name)
(new_expression constructor: (member_expression object: (_) @call.receiver property: (property_identifier) @call.name))

; <Component /> usage
((jsx_opening_element name: (identifier) @call.name) (#match? @call.name "^[A-Z]") (#set! edge "renders"))
((jsx_self_closing_element name: (identifier) @call.name) (#match? @call.name "^[A-Z]") (#set! edge "renders"))

; ── Imports ─────────────────────────────────────────────────────────────

(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name !alias))) source: (string) @import.path)
(import_statement (import_clause (named_imports (import_specifier name: (identifier) @import.name alias: (identifier) @import.alias))) source: (string) @import.path)
(import_statement (import_clause (identifier) @import.default) source: (string) @import.path)
(import_statement (import_clause (namespace_import (identifier) @import.alias)) source: (string) @import.path)

; const x = require("./x")
((variable_declarator
  name: (identifier) @import.alias
  value: (call_expression function: (identifier) @_req arguments: (arguments . (string) @import.path)))
  (#eq? @_req "require"))
; const { a, b } = require("./x")
((variable_declarator
  name: (object_pattern (shorthand_property_identifier_pattern) @import.name)
  value: (call_expression function: (identifier) @_req arguments: (arguments . (string) @import.path)))
  (#eq? @_req "require"))

; ── Local variable types ────────────────────────────────────────────────

(variable_declarator name: (identifier) @binding.name value: (new_expression constructor: (_) @binding.type))
(variable_declarator name: (identifier) @binding.name value: (call_expression function: (_) @binding.call))
(variable_declarator name: (identifier) @binding.name value: (await_expression (call_expression function: (_) @binding.call)))
